//! 12-band Linkwitz-Riley (LR4) crossover tree that sums back flat.
//!
//! `band[k] = LP4_k(rest)`, `rest = HP4_k(rest)` for k = 0..10, `band[11] = rest`.
//! Each `band[k]` (k < 10) then passes allpass filters at crossovers j = k+1..10
//! so every band shares the phase response of the full tree.

use crate::bands::{BAND_COUNT, crossovers};

const Q: f64 = std::f64::consts::FRAC_1_SQRT_2;
const XOVERS: usize = BAND_COUNT - 1;
/// Crossovers must lie below this fraction of the sample rate.
const MAX_XOVER_FRACTION: f64 = 0.45;

/// Per-band output buffers, allocated once.
pub struct BandBuffers {
    pub bands: Vec<Vec<f32>>,
}

impl BandBuffers {
    pub fn new(max_frames: usize) -> Self {
        Self { bands: (0..BAND_COUNT).map(|_| vec![0.0; max_frames]).collect() }
    }
}

use crate::biquad::Biquad;

pub struct Filterbank {
    /// Two cascaded sections per crossover (LP): index 2*k, 2*k+1.
    lp: Vec<Biquad>,
    hp: Vec<Biquad>,
    /// Allpass for band k at crossover j (j > k): index k*XOVERS + j.
    ap: Vec<Biquad>,
}

impl Filterbank {
    pub fn new(fs: f64) -> Self {
        let xs = crossovers();
        // A crossover at or above MAX_XOVER_FRACTION * fs would make the RBJ filters
        // unstable. Such a crossover becomes a pass-through for the low side (identity),
        // zero for the high side, and has no allpass compensation, so every band above
        // merges into the highest valid band and the bands above that are silent.
        let valid = |f: f64| f < MAX_XOVER_FRACTION * fs;
        let mut lp = Vec::with_capacity(2 * XOVERS);
        let mut hp = Vec::with_capacity(2 * XOVERS);
        for &f in &xs {
            for _ in 0..2 {
                if valid(f) {
                    lp.push(Biquad::lowpass(f, fs, Q));
                    hp.push(Biquad::highpass(f, fs, Q));
                } else {
                    lp.push(Biquad::identity());
                    hp.push(Biquad::zero());
                }
            }
        }
        let mut ap = Vec::with_capacity(XOVERS * XOVERS);
        for _k in 0..XOVERS {
            for &f in &xs {
                ap.push(if valid(f) { Biquad::allpass(f, fs, Q) } else { Biquad::identity() });
            }
        }
        Self { lp, hp, ap }
    }

    /// Splits `input` into `out.bands[i][..input.len()]`. No allocation.
    /// Panics (assert) if `input.len()` exceeds the capacity of `out`.
    pub fn split(&mut self, input: &[f32], out: &mut BandBuffers) {
        let n = input.len();
        assert!(out.bands.len() == BAND_COUNT && out.bands.iter().all(|b| b.len() >= n));
        for (i, &x) in input.iter().enumerate() {
            let mut rest = x;
            for k in 0..XOVERS {
                let mut low = self.lp[2 * k].process(rest);
                low = self.lp[2 * k + 1].process(low);
                rest = self.hp[2 * k].process(rest);
                rest = self.hp[2 * k + 1].process(rest);
                for j in (k + 1)..XOVERS {
                    low = self.ap[k * XOVERS + j].process(low);
                }
                out.bands[k][i] = low;
            }
            out.bands[BAND_COUNT - 1][i] = rest;
        }
    }

    pub fn reset(&mut self) {
        for b in self.lp.iter_mut().chain(self.hp.iter_mut()).chain(self.ap.iter_mut()) {
            b.reset();
        }
    }
}
