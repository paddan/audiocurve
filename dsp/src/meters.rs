//! Level meters: `MeterState` runs on the audio thread, `AtomicMeters` is shared.

use crate::params::Meters;
use std::sync::atomic::{AtomicU32, Ordering};

const RMS_TAU_S: f64 = 0.3;
const PEAK_FALL_DB: f32 = 120.0;
const PEAK_FALL_S: f64 = 1.5;
const FLOOR_DB: f32 = -120.0;

pub struct MeterState {
    power: [f32; 2],
    peak_db: [f32; 2],
    alpha: f32,
    fall_per_sample: f32,
}

fn to_db(v: f32, scale: f32) -> f32 {
    (scale * v.max(1e-30).log10()).max(FLOOR_DB)
}

impl MeterState {
    pub fn new(fs: f64) -> Self {
        Self {
            power: [0.0; 2],
            peak_db: [FLOOR_DB; 2],
            alpha: (1.0 - (-1.0 / (RMS_TAU_S * fs)).exp()) as f32,
            fall_per_sample: (PEAK_FALL_DB as f64 / (PEAK_FALL_S * fs)) as f32,
        }
    }

    /// Returns (rms dBFS, peak dBFS) per channel (full-scale sine = 0 dBFS).
    pub fn update(&mut self, l: &[f32], r: &[f32]) -> ([f32; 2], [f32; 2]) {
        debug_assert_eq!(l.len(), r.len());
        let mut rms = [0.0; 2];
        let mut peak = [0.0; 2];
        for (ch, x) in [l, r].into_iter().enumerate() {
            let mut p = self.power[ch];
            let mut m = 0.0f32;
            for &s in x {
                p += self.alpha * (s * s - p);
                m = m.max(s.abs());
            }
            self.power[ch] = p;
            let held = self.peak_db[ch] - self.fall_per_sample * x.len() as f32;
            self.peak_db[ch] = to_db(m, 20.0).max(held).max(FLOOR_DB);
            rms[ch] = to_db(2.0 * p, 10.0);
            peak[ch] = self.peak_db[ch];
        }
        (rms, peak)
    }
}

/// Meter values shared across threads (f32 bits in relaxed atomics).
pub struct AtomicMeters {
    v: [AtomicU32; 8],
}

impl Default for AtomicMeters {
    fn default() -> Self {
        Self::new()
    }
}

impl AtomicMeters {
    pub fn new() -> Self {
        Self { v: std::array::from_fn(|_| AtomicU32::new(FLOOR_DB.to_bits())) }
    }

    fn store4(&self, base: usize, rms: [f32; 2], peak: [f32; 2]) {
        let vals = [rms[0], rms[1], peak[0], peak[1]];
        for (i, x) in vals.into_iter().enumerate() {
            self.v[base + i].store(x.to_bits(), Ordering::Relaxed);
        }
    }

    pub fn store_in(&self, rms: [f32; 2], peak: [f32; 2]) {
        self.store4(0, rms, peak);
    }

    pub fn store_out(&self, rms: [f32; 2], peak: [f32; 2]) {
        self.store4(4, rms, peak);
    }

    pub fn load(&self) -> Meters {
        let g = |i: usize| f32::from_bits(self.v[i].load(Ordering::Relaxed));
        Meters {
            in_rms_db: [g(0), g(1)],
            in_peak_db: [g(2), g(3)],
            out_rms_db: [g(4), g(5)],
            out_peak_db: [g(6), g(7)],
        }
    }
}
