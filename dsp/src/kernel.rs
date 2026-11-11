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

struct Ear {
    bank: Filterbank,
    bufs: BandBuffers,
    comps: Vec<BandCompressor>,
}

impl Ear {
    fn new(fs: f64, max_frames: usize) -> Self {
        Self {
            bank: Filterbank::new(fs),
            bufs: BandBuffers::new(max_frames),
            comps: (0..BAND_COUNT).map(|_| BandCompressor::new(fs)).collect(),
        }
    }

    fn reset(&mut self) {
        self.bank.reset();
        for c in &mut self.comps {
            c.reset();
        }
    }

    /// Split, compress every band and sum back into `x` (len <= max_frames).
    fn run(&mut self, x: &mut [f32], fits: &[crate::params::BandFitting; BAND_COUNT], attack: f64, release: f64) {
        self.bank.split(x, &mut self.bufs);
        let n = x.len();
        x.fill(0.0);
        for (band, (comp, fit)) in self.bufs.bands.iter_mut().zip(self.comps.iter_mut().zip(fits)) {
            let b = &mut band[..n];
            comp.process(b, fit, attack, release);
            for (o, v) in x.iter_mut().zip(b.iter()) {
                *o += *v;
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
            self.ears[0].run(cl, &p.left, p.attack_ms, p.release_ms);
            self.ears[1].run(cr, &p.right, p.attack_ms, p.release_ms);
            self.apply_output_gain(p.output_gain_db, cl, cr);
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
        Some(Box::new(Handle {
            params: TripleBuffer::new(Params::unity()),
            meters: AtomicMeters::new(),
            dsp: UnsafeCell::new(Dsp::new(fs, max_frames)),
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
        // SAFETY: only reads the limiter's immutable latency.
        unsafe { (*self.dsp.get()).limiter.latency_frames() as u32 }
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
