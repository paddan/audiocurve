//! Processing chain (`Dsp`) and the FFI object (`Handle`).

use crate::bands::BAND_COUNT;
use crate::classifier::Classifier;
use crate::compressor::BandCompressor;
use crate::filterbank::{BandBuffers, Filterbank};
use crate::limiter::{soft_knee_db, Limiter};
use crate::meters::{AtomicMeters, MeterState, ReductionMeter};
use crate::params::{Meters, Params};
use crate::spectrum::{Capture, Reader, Spectrum};
use crate::triple::TripleBuffer;
use std::cell::UnsafeCell;
use std::sync::Mutex;

/// Output-gain smoothing time constant: 20/3 ms.
const GAIN_TAU_S: f64 = 0.020 / 3.0;
/// Speech and Music time constants; the speech weight glides (log-wise) between them.
const SPEECH_ATTACK_MS: f64 = 5.0;
const SPEECH_RELEASE_MS: f64 = 50.0;
const MUSIC_ATTACK_MS: f64 = 10.0;
const MUSIC_RELEASE_MS: f64 = 200.0;

/// Geometric blend: `a` at s = 0, `b` at s = 1.
#[inline]
fn log_blend(a: f64, b: f64, s: f64) -> f64 {
    a * (b / a).powf(s.clamp(0.0, 1.0))
}
/// The summed band-output budget sits this far below the MPO. It leaves room for the
/// crest factor of real signals (the budget is a short-term power level), so the
/// broadband limiter only catches brief peaks instead of ducking the whole mix.
const BAND_CEILING_MARGIN_DB: f64 = 6.0;
/// The bands are delayed by this much after analysis, so the budget attenuation has
/// ramped in by the time a peak reaches the output (no overshoot for the broadband
/// limiter to clamp, which is heard as distortion on dense treble).
const BUDGET_LOOKAHEAD_S: f64 = 0.003;
/// Smoothing of the per-band budget attenuation; the attack completes within the lookahead.
const BUDGET_ATTACK_S: f64 = BUDGET_LOOKAHEAD_S / 3.0;
const BUDGET_RELEASE_S: f64 = 0.3;
/// Width of the soft knee around the budget (dB): the summed band output is eased
/// toward the budget from KNEE/2 below it instead of hitting a hard wall.
const BUDGET_KNEE_DB: f64 = 6.0;


struct Ear {
    /// Budget attenuation per band (dB, >= 0), smoothed so it never steps from one sample
    /// to the next: an instant per-sample cap modulates dense treble (distorted guitar,
    /// sibilants) and is heard as distortion.
    atten: [f64; BAND_COUNT],
    atten_attack: f64,
    atten_release: f64,
    /// Lookahead delay lines for the band signal and its WDRC gain (one ring per band).
    delay_band: Vec<Vec<f32>>,
    delay_gain: Vec<Vec<f32>>,
    delay_pos: usize,
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
            atten: [0.0; BAND_COUNT],
            atten_attack: 1.0 - (-1.0 / (BUDGET_ATTACK_S * fs)).exp(),
            atten_release: 1.0 - (-1.0 / (BUDGET_RELEASE_S * fs)).exp(),
            delay_band: (0..BAND_COUNT).map(|_| vec![0.0; Self::lookahead(fs)]).collect(),
            delay_gain: (0..BAND_COUNT).map(|_| vec![0.0; Self::lookahead(fs)]).collect(),
            delay_pos: 0,
            bank: Filterbank::new(fs),
            bufs: BandBuffers::new(max_frames),
            comps: (0..BAND_COUNT).map(|_| BandCompressor::new(fs)).collect(),
            gains: (0..BAND_COUNT).map(|_| vec![0.0; max_frames]).collect(),
            levels: (0..BAND_COUNT).map(|_| vec![0.0; max_frames]).collect(),
        }
    }

    /// Lookahead in frames (at least 1).
    fn lookahead(fs: f64) -> usize {
        ((BUDGET_LOOKAHEAD_S * fs).round() as usize).max(1)
    }

    fn reset(&mut self) {
        self.atten = [0.0; BAND_COUNT];
        for d in self.delay_band.iter_mut().chain(self.delay_gain.iter_mut()) {
            d.fill(0.0);
        }
        self.delay_pos = 0;
        self.bank.reset();
        for c in &mut self.comps {
            c.reset();
        }
    }

    /// Split, compress every band and sum back into `x` (len <= max_frames).
    /// `ceiling_dbfs` is the total output budget of all bands together (same dB scale
    /// as the band detectors); when the bands' predicted output exceeds it, the loudest
    /// bands are capped (water-filling) and the rest pass unchanged.
    /// Returns the deepest broadband-equivalent budget reduction in this block (dB, >= 0).
    fn run(&mut self, x: &mut [f32], speech: &[crate::params::BandFitting; BAND_COUNT],
           music: &[crate::params::BandFitting; BAND_COUNT], weight: f64, attack: f64, release: f64, ceiling_dbfs: f64) -> f32 {
        self.bank.split(x, &mut self.bufs);
        let n = x.len();
        for i in 0..BAND_COUNT {
            self.comps[i].analyze(&self.bufs.bands[i][..n], &speech[i], &music[i], weight, attack, release,
                                  &mut self.gains[i][..n], &mut self.levels[i][..n]);
        }
        let budget_db = ceiling_dbfs;
        // Below the knee's start (budget - knee/2) nothing is touched: cheap test before any log.
        let knee_floor = 10f64.powf((budget_db - BUDGET_KNEE_DB / 2.0) / 10.0);
        let mut p = [0f64; BAND_COUNT];
        let (ca, cr) = (self.atten_attack, self.atten_release);
        let dlen = self.delay_band[0].len();
        let mut max_red = 0.0f64;
        for t in 0..n {
            let d = self.delay_pos;
            let mut total = 0.0;
            for i in 0..BAND_COUNT {
                p[i] = 10f64.powf((self.levels[i][t] + self.gains[i][t]) as f64 / 10.0);
                total += p[i];
            }
            let cap = if total > knee_floor {
                let t = soft_knee_db(10.0 * total.log10(), budget_db, BUDGET_KNEE_DB);
                water_level(&p, 10f64.powf(t / 10.0))
            } else {
                f64::INFINITY
            };
            let mut y = 0.0f64;
            for i in 0..BAND_COUNT {
                let target = if p[i] > cap { 10.0 * (p[i] / cap).log10() } else { 0.0 };
                let a = &mut self.atten[i];
                *a += (target - *a) * if target > *a { ca } else { cr };
                // Apply to the delayed sample: the attenuation already saw what is coming.
                let (xb, gb) = (self.delay_band[i][d], self.delay_gain[i][d]);
                self.delay_band[i][d] = self.bufs.bands[i][t];
                self.delay_gain[i][d] = self.gains[i][t];
                let g = gb as f64 - *a;
                y += xb as f64 * 10f64.powf(g / 20.0);
            }
            x[t] = y as f32;
            // Metering only: total predicted power before vs after the attenuation, sampled
            // every 16th frame to keep the extra exp() calls cheap.
            if t % 16 == 0 && total > 0.0 {
                let after: f64 = (0..BAND_COUNT).map(|i| p[i] * 10f64.powf(-self.atten[i] / 10.0)).sum();
                if after > 0.0 {
                    max_red = max_red.max(10.0 * (total / after).log10());
                }
            }
            self.delay_pos = if d + 1 == dlen { 0 } else { d + 1 };
        }
        max_red as f32
    }
}

/// A gain in dB, smoothed per sample (tau 20/3 ms) so changes never click.
struct SmoothGain {
    db: f64,
    init: bool,
    alpha: f64,
}

impl SmoothGain {
    fn new(fs: f64) -> Self {
        Self { db: 0.0, init: false, alpha: 1.0 - (-1.0 / (GAIN_TAU_S * fs)).exp() }
    }

    fn reset(&mut self) {
        self.init = false;
    }

    fn apply(&mut self, target_db: f64, l: &mut [f32], r: &mut [f32]) {
        if !self.init {
            self.db = target_db;
            self.init = true;
        }
        if self.db == target_db && target_db == 0.0 {
            return;
        }
        for (a, b) in l.iter_mut().zip(r.iter_mut()) {
            if (target_db - self.db).abs() > 1e-6 {
                self.db += self.alpha * (target_db - self.db);
            } else {
                self.db = target_db;
            }
            let g = 10f64.powf(self.db / 20.0) as f32;
            *a *= g;
            *b *= g;
        }
    }
}

/// Crossfade between the processed signal and the input delayed by the kernel latency,
/// so turning processing on or off never clicks and never changes the timing.
struct Activity {
    ring: [Vec<f32>; 2],
    pos: usize,
    dry: [Vec<f32>; 2],
    g: f32,
    step: f32,
    init: bool,
}

impl Activity {
    const FADE_S: f64 = 0.050;

    fn new(fs: f64, max_frames: usize, latency: usize) -> Self {
        Self {
            ring: [vec![0.0; latency + 1], vec![0.0; latency + 1]],
            pos: 0,
            dry: [vec![0.0; max_frames], vec![0.0; max_frames]],
            g: 1.0,
            step: (1.0 / (Self::FADE_S * fs)) as f32,
            init: false,
        }
    }

    fn reset(&mut self) {
        for r in &mut self.ring {
            r.fill(0.0);
        }
        self.pos = 0;
        self.init = false;
    }

    /// Keep a copy of the (sanitized) input before processing.
    fn capture(&mut self, l: &[f32], r: &[f32]) {
        self.dry[0][..l.len()].copy_from_slice(l);
        self.dry[1][..r.len()].copy_from_slice(r);
    }

    /// Mix the processed `l`/`r` with the delayed dry input.
    fn mix(&mut self, active: bool, l: &mut [f32], r: &mut [f32]) {
        let target = if active { 1.0 } else { 0.0 };
        if !self.init {
            self.g = target;
            self.init = true;
        }
        let len = self.ring[0].len();
        for i in 0..l.len() {
            let p = self.pos;
            self.ring[0][p] = self.dry[0][i];
            self.ring[1][p] = self.dry[1][i];
            let o = if p + 1 == len { 0 } else { p + 1 };
            let (dl, dr) = (self.ring[0][o], self.ring[1][o]);
            self.pos = o;
            if self.g != target {
                self.g = if self.g < target { (self.g + self.step).min(target) } else { (self.g - self.step).max(target) };
            }
            let g = self.g;
            if g < 1.0 {
                l[i] = g * l[i] + (1.0 - g) * dl;
                r[i] = g * r[i] + (1.0 - g) * dr;
            }
        }
    }
}

struct Dsp {
    max_frames: usize,
    ears: [Ear; 2],
    limiter: Limiter,
    in_meter: MeterState,
    out_meter: MeterState,
    reduction: ReductionMeter,
    classifier: Classifier,
    input_gain: SmoothGain,
    output_gain: SmoothGain,
    activity: Activity,
}

impl Dsp {
    fn new(fs: f64, max_frames: usize) -> Self {
        let limiter = Limiter::new(fs, max_frames);
        let latency = limiter.latency_frames() + Ear::lookahead(fs);
        Self {
            max_frames,
            ears: [Ear::new(fs, max_frames), Ear::new(fs, max_frames)],
            limiter,
            in_meter: MeterState::new(fs),
            out_meter: MeterState::new(fs),
            reduction: ReductionMeter::new(fs),
            classifier: Classifier::new(fs),
            input_gain: SmoothGain::new(fs),
            output_gain: SmoothGain::new(fs),
            activity: Activity::new(fs, max_frames, latency),
        }
    }

    fn reset(&mut self) {
        self.reduction.reset();
        self.activity.reset();
        self.classifier.reset();
        for e in &mut self.ears {
            e.reset();
        }
        self.limiter.reset();
        self.input_gain.reset();
        self.output_gain.reset();
    }

    fn process(&mut self, p: &Params, meters: &AtomicMeters, capture: &Capture, l: &mut [f32], r: &mut [f32]) {
        // Output gain is applied after the bands, so it comes off the band ceiling.
        let limiter_on = p.limiter_enabled >= 0.5;
        // With the limiter off the band budget is off too (an infinite ceiling never engages).
        let band_ceiling = if limiter_on {
            p.mpo_dbfs.min(-1.0) - BAND_CEILING_MARGIN_DB - p.output_gain_db
        } else {
            f64::INFINITY
        };
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
            self.activity.capture(cl, cr);
            // Input level first: the In meter and spectrum show what the bands actually get.
            self.input_gain.apply(p.input_gain_db, cl, cr);
            let base = capture.position();
            capture.put_in(base, cl, cr);
            let (rms, peak) = self.in_meter.update(cl, cr);
            meters.store_in(rms, peak);
            self.classifier.process(cl, cr);
            let speech = self.classifier.speech() as f64;
            meters.store_speech(speech as f32);
            let weight = if p.auto_mode >= 0.5 { speech } else { p.speech_weight.clamp(0.0, 1.0) };
            let attack = log_blend(MUSIC_ATTACK_MS, SPEECH_ATTACK_MS, weight);
            let release = log_blend(MUSIC_RELEASE_MS, SPEECH_RELEASE_MS, weight);
            // pre_process: identity (reserved for v2)
            let red_l = self.ears[0].run(cl, &p.left, &p.music_left, weight, attack, release, band_ceiling);
            let red_r = self.ears[1].run(cr, &p.right, &p.music_right, weight, attack, release, band_ceiling);
            self.output_gain.apply(p.output_gain_db, cl, cr);
            // Defence in depth: never feed a non-finite value to the limiter. Drop the
            // chunk and restart every filter/envelope from a clean state.
            if cl.iter().chain(cr.iter()).any(|v| !v.is_finite()) {
                self.reset();
                cl.fill(0.0);
                cr.fill(0.0);
            }
            if limiter_on {
                self.limiter.process(cl, cr, p.mpo_dbfs);
            } else {
                self.limiter.process_bypass(cl, cr);
            }
            self.activity.mix(p.active >= 0.5, cl, cr);
            let (rms, peak) = self.out_meter.update(cl, cr);
            meters.store_out(rms, peak);
            let lim = self.limiter.last_reduction_db();
            let red = self.reduction.update([red_l + lim[0], red_r + lim[1]], cl.len());
            meters.store_reduction(red);
            capture.put_out(base, cl, cr);
            capture.publish(base, cl.len());
        }
    }
}

/// FFI object (`ac_kernel`).
pub struct Handle {
    params: TripleBuffer<Params>,
    meters: AtomicMeters,
    capture: Capture,
    reader: Mutex<Reader>,
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
        let latency = (dsp.limiter.latency_frames() + Ear::lookahead(fs)) as u32;
        Some(Box::new(Handle {
            params: TripleBuffer::new(Params::unity()),
            meters: AtomicMeters::new(),
            capture: Capture::new(),
            reader: Mutex::new(Reader::new(fs)),
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
        dsp.process(&p, &self.meters, &self.capture, &mut l[..n], &mut r[..n]);
    }

    pub fn latency_frames(&self) -> u32 {
        self.latency
    }

    pub fn meters(&self) -> Meters {
        self.meters.load()
    }

    /// Latest input/output spectrum (UI thread; allocation-free after construction, but it
    /// takes a mutex and runs four FFTs, so never call it from the audio thread).
    pub fn read_spectrum(&self, out: &mut Spectrum) {
        let mut r = self.reader.lock().unwrap_or_else(|e| e.into_inner());
        r.read(&self.capture, out);
    }

    pub fn spectrum(&self) -> Spectrum {
        let mut s = Spectrum::default();
        self.read_spectrum(&mut s);
        s
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
    fn soft_knee_shape() {
        assert_eq!(soft_knee_db(-20.0, -10.0, 6.0), -20.0);
        assert_eq!(soft_knee_db(-13.0, -10.0, 6.0), -13.0);
        assert!((soft_knee_db(-7.0, -10.0, 6.0) + 10.0).abs() < 1e-12);
        assert_eq!(soft_knee_db(0.0, -10.0, 6.0), -10.0);
        let mid = soft_knee_db(-10.0, -10.0, 6.0);
        assert!((mid + 10.75).abs() < 1e-12, "{mid}");
        let mut prev = f64::NEG_INFINITY;
        for i in 0..200 {
            let y = soft_knee_db(-20.0 + i as f64 * 0.1, -10.0, 6.0);
            assert!(y >= prev && y <= -10.0);
            prev = y;
        }
    }

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
