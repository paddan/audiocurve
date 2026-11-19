//! The DSP engine the app owns: a thin wrapper over the kernel handle.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use audiocurve_dsp::{Handle, Meters, Params, Spectrum};

pub struct Engine {
    handle: Box<Handle>,
    max_frames: usize,
    sample_rate: f64,
    /// The last `set_params` (UI thread only; the IO thread never touches it).
    last_params: Mutex<Params>,
}

impl Engine {
    /// Panics on invalid arguments (the callers pass 48 000 / 4096).
    pub fn new(sample_rate: f64, max_frames: usize) -> Engine {
        let handle = Handle::new(sample_rate, max_frames).unwrap_or_else(|| {
            panic!("invalid engine arguments: {sample_rate} Hz, {max_frames} frames")
        });
        Engine {
            handle,
            max_frames,
            sample_rate,
            last_params: Mutex::new(Params::unity()),
        }
    }

    /// A new engine at `sample_rate`, same block size, carrying `last_params`.
    pub fn with_sample_rate(&self, sample_rate: f64) -> Engine {
        let e = Engine::new(sample_rate, self.max_frames);
        e.set_params(&self.last_params());
        e
    }

    pub fn sample_rate(&self) -> f64 {
        self.sample_rate
    }

    /// The params of the last `set_params`, or unity before the first.
    pub fn last_params(&self) -> Params {
        *self.last_params.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The largest block `process` accepts.
    pub fn max_frames(&self) -> usize {
        self.max_frames
    }

    pub fn set_params(&self, p: &Params) {
        // Holding the lock also keeps the kernel's params single-writer.
        let mut last = self.last_params.lock().unwrap_or_else(|e| e.into_inner());
        *last = *p;
        self.handle.set_params(p);
    }

    pub fn meters(&self) -> Meters {
        self.handle.meters()
    }

    pub fn spectrum(&self) -> Spectrum {
        self.handle.spectrum()
    }

    /// # Safety
    /// Call from at most one thread at a time, with at most `max_frames()` frames.
    pub unsafe fn process(&self, l: &mut [f32], r: &mut [f32]) {
        // SAFETY: forwarded caller contract.
        unsafe { self.handle.process(l, r) }
    }
}

/// The model swaps the inner `Arc` on reload; a route builder clones the current one at build time.
pub type SharedEngine = Rc<RefCell<Arc<Engine>>>;
