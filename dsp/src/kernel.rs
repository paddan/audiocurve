//! Processing chain (`Dsp`) and the FFI object (`Handle`).

use crate::bands::BAND_COUNT;
use crate::compressor::BandCompressor;
use crate::filterbank::{BandBuffers, Filterbank};
use crate::limiter::Limiter;
use crate::meters::{AtomicMeters, MeterState};
use crate::params::{Meters, Params};
use crate::triple::TripleBuffer;
use std::cell::UnsafeCell;

/// Output-gain smoothing time constant: 20/3 ms.
const GAIN_TAU_S: f64 = 0.020 / 3.0;
/// The summed band-output budget sits this far below the MPO. It leaves room for the
/// crest factor of real signals (the budget is a short-term power level), so the
/// broadband limiter only catches brief peaks instead of ducking the whole mix.
const BAND_CEILING_MARGIN_DB: f64 = 9.0;

struct Ear {
    bank: Filterbank,
    bufs: BandBuffers,
    comps: Vec<BandCompressor>,
    /// Per-band, per-sample WDRC gain (dB) from the analysis pass.
    gains: Vec<Vec<f32>>,
    /// Per-band, per-sample MPO detector level (dB).
    levels: Vec<Vec<f32>>,
}

/// Water-filling: given band output powers `p` (linear) whose sum exceeds `budget`,
/// returns the per-band cap `c` such that `sum(min(p_i, c)) == budget`. Bands below `c`
/// are untouched, so a loud band is limited without ducking the quiet ones.
fn water_level(p: &[f64; BAND_COUNT], budget: f64) -> f64 {
    let mut s = *p;
    // Descending insertion sort (12 elements, no allocation).
    for i in 1..BAND_COUNT {
        let mut j = i;
        while j > 0 && s[j - 1] < s[j] {
            s.swap(j - 1, j);
            j -= 1;
        }
    }
    // With the k largest bands capped at c: k*c + sum(rest) = budget.
    let mut rest: f64 = s.iter().sum();
    for k in 1..=BAND_COUNT {
        rest -= s[k - 1];
        let c = (budget - rest) / k as f64;
        let next = if k < BAND_COUNT { s[k] } else { 0.0 };
        if c >= next {
            return c.max(0.0);
        }
    }
    0.0
}

impl Ear {
    fn new(fs: f64, max_frames: usize) -> Self {
        Self {
            bank: Filterbank::new(fs),
            bufs: BandBuffers::new(max_frames),
            comps: (0..BAND_COUNT).map(|_| BandCompressor::new(fs)).collect(),
            gains: (0..BAND_COUNT).map(|_| vec![0.0; max_frames]).collect(),
            levels: (0..BAND_COUNT).map(|_| vec![0.0; max_frames]).collect(),
        }
    }

    fn reset(&mut self) {
        self.bank.reset();
        for c in &mut self.comps {
            c.reset();
        }
    }

    /// Split, compress every band and sum back into `x` (len <= max_frames).
    /// `ceiling_dbfs` is the total output budget of all bands together (same dB scale
    /// as the band detectors); when the bands' predicted output exceeds it, the loudest
    /// bands are capped (water-filling) and the rest pass unchanged.
    fn run(&mut self, x: &mut [f32], fits: &[crate::params::BandFitting; BAND_COUNT], attack: f64, release: f64, ceiling_dbfs: f64) {
        self.bank.split(x, &mut self.bufs);
        let n = x.len();
        for i in 0..BAND_COUNT {
            self.comps[i].analyze(&self.bufs.bands[i][..n], &fits[i], attack, release,
                                  &mut self.gains[i][..n], &mut self.levels[i][..n]);
        }
        let budget = 10f64.powf(ceiling_dbfs / 10.0);
        let mut p = [0f64; BAND_COUNT];
        for t in 0..n {
            let mut total = 0.0;
            for i in 0..BAND_COUNT {
                p[i] = 10f64.powf((self.levels[i][t] + self.gains[i][t]) as f64 / 10.0);
                total += p[i];
            }
            let cap = if total > budget { water_level(&p, budget) } else { f64::INFINITY };
            let mut y = 0.0f64;
            for i in 0..BAND_COUNT {
                let mut g = self.gains[i][t] as f64;
                if p[i] > cap {
                    g -= 10.0 * (p[i] / cap).log10();
                }
                y += self.bufs.bands[i][t] as f64 * 10f64.powf(g / 20.0);
            }
            x[t] = y as f32;
        }
    }
}

struct Dsp {
    max_frames: usize,
    ears: [Ear; 2],
    limiter: Limiter,
    in_meter: MeterState,
    out_meter: MeterState,
    gain_db: f64,
    gain_init: bool,
    gain_alpha: f64,
}

impl Dsp {
    fn new(fs: f64, max_frames: usize) -> Self {
        Self {
            max_frames,
            ears: [Ear::new(fs, max_frames), Ear::new(fs, max_frames)],
            limiter: Limiter::new(fs, max_frames),
            in_meter: MeterState::new(fs),
            out_meter: MeterState::new(fs),
            gain_db: 0.0,
            gain_init: false,
            gain_alpha: 1.0 - (-1.0 / (GAIN_TAU_S * fs)).exp(),
        }
    }

    fn reset(&mut self) {
        for e in &mut self.ears {
            e.reset();
        }
        self.limiter.reset();
        self.gain_init = false;
    }

    fn apply_output_gain(&mut self, target_db: f64, l: &mut [f32], r: &mut [f32]) {
        if !self.gain_init {
            self.gain_db = target_db;
            self.gain_init = true;
        }
        for (a, b) in l.iter_mut().zip(r.iter_mut()) {
            if (target_db - self.gain_db).abs() > 1e-6 {
                self.gain_db += self.gain_alpha * (target_db - self.gain_db);
            } else {
                self.gain_db = target_db;
            }
            let g = 10f64.powf(self.gain_db / 20.0) as f32;
            *a *= g;
            *b *= g;
        }
    }

    fn process(&mut self, p: &Params, meters: &AtomicMeters, l: &mut [f32], r: &mut [f32]) {
        // Output gain is applied after the bands, so it comes off the band ceiling.
        let band_ceiling = p.mpo_dbfs.min(-1.0) - BAND_CEILING_MARGIN_DB - p.output_gain_db;
        let mut pos = 0;
        let total = l.len();
        while pos < total {
            let end = (pos + self.max_frames).min(total);
            let (cl, cr) = (&mut l[pos..end], &mut r[pos..end]);
            pos = end;
            for s in cl.iter_mut().chain(cr.iter_mut()) {
                if !s.is_finite() {
                    *s = 0.0;
                }
            }
            let (rms, peak) = self.in_meter.update(cl, cr);
            meters.store_in(rms, peak);
            // pre_process: identity (reserved for v2)
            self.ears[0].run(cl, &p.left, p.attack_ms, p.release_ms, band_ceiling);
            self.ears[1].run(cr, &p.right, p.attack_ms, p.release_ms, band_ceiling);
            self.apply_output_gain(p.output_gain_db, cl, cr);
            // Defence in depth: never feed a non-finite value to the limiter. Drop the
            // chunk and restart every filter/envelope from a clean state.
            if cl.iter().chain(cr.iter()).any(|v| !v.is_finite()) {
                self.reset();
                cl.fill(0.0);
                cr.fill(0.0);
            }
            self.limiter.process(cl, cr, p.mpo_dbfs);
            let (rms, peak) = self.out_meter.update(cl, cr);
            meters.store_out(rms, peak);
        }
    }
}

/// FFI object (`ac_kernel`).
pub struct Handle {
    params: TripleBuffer<Params>,
    meters: AtomicMeters,
    dsp: UnsafeCell<Dsp>,
    latency: u32,
}

// SAFETY: `params` is single-writer/single-reader (see `set_params`/`process`), `meters`
// is atomic, and `dsp` is only touched from the (single) audio thread via the unsafe
// `process`/`reset` whose contract requires that.
unsafe impl Sync for Handle {}
unsafe impl Send for Handle {}

impl Handle {
    pub fn new(fs: f64, max_frames: usize) -> Option<Box<Handle>> {
        if !(fs.is_finite() && (8000.0..=384000.0).contains(&fs)) || !(1..=16384).contains(&max_frames) {
            return None;
        }
        let dsp = Dsp::new(fs, max_frames);
        let latency = dsp.limiter.latency_frames() as u32;
        Some(Box::new(Handle {
            params: TripleBuffer::new(Params::unity()),
            meters: AtomicMeters::new(),
            dsp: UnsafeCell::new(dsp),
            latency,
        }))
    }

    /// Must be called from one thread at a time.
    pub fn set_params(&self, p: &Params) {
        self.params.write(*p);
    }

    /// Processes `min(l.len(), r.len())` frames in place.
    ///
    /// # Safety
    /// Call from at most one thread at a time (the audio thread); not concurrently with `reset`.
    pub unsafe fn process(&self, l: &mut [f32], r: &mut [f32]) {
        debug_assert_eq!(l.len(), r.len());
        let n = l.len().min(r.len());
        let p = self.params.read();
        // SAFETY: exclusive access guaranteed by the caller contract.
        let dsp = unsafe { &mut *self.dsp.get() };
        dsp.process(&p, &self.meters, &mut l[..n], &mut r[..n]);
    }

    pub fn latency_frames(&self) -> u32 {
        self.latency
    }

    pub fn meters(&self) -> Meters {
        self.meters.load()
    }

    /// # Safety
    /// Same contract as `process`.
    pub unsafe fn reset(&self) {
        // SAFETY: exclusive access guaranteed by the caller contract.
        unsafe { (*self.dsp.get()).reset() };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn water_level_caps_only_loud_bands() {
        let mut p = [1.0f64; BAND_COUNT];
        p[10] = 100.0;
        p[11] = 50.0;
        // Ten quiet bands use 10 of a budget of 30, so the two loud ones share 20.
        let c = water_level(&p, 30.0);
        assert!((c - 10.0).abs() < 1e-9, "{c}");
        let used: f64 = p.iter().map(|&v| v.min(c)).sum();
        assert!((used - 30.0).abs() < 1e-9);
    }

    #[test]
    fn water_level_when_budget_below_quiet_floor() {
        let p = [4.0f64; BAND_COUNT];
        let c = water_level(&p, 12.0);
        assert!((c - 1.0).abs() < 1e-9, "{c}");
    }
}
