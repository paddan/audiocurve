//! Per-band wide-dynamic-range compressor. The level-to-gain mapping is the static
//! curve `params::gain_db`; this module adds the level detector and gain smoothing.
//!
//! Detector: symmetric power average (5 ms), level in dB, then attack/release
//! smoothing of the dB level. Applied gain is smoothed in dB with 20/3 ms.

use crate::params::{BandFitting, gain_db};

/// Power averaging time constant (symmetric rise and fall).
const POWER_TAU_S: f64 = 0.005;
/// Applied-gain smoothing time constant is 20/3 ms.
const GAIN_TAU_S: f64 = 0.020 / 3.0;
/// Initial level in dB; low enough that silence stays silent.
const LEVEL_FLOOR_DB: f64 = -120.0;
/// Release time constant of the band-MPO level envelope (instant attack).
const MPO_RELEASE_S: f64 = 0.100;

pub struct BandCompressor {
    fs: f64,
    power: f64,
    level_db: f64,
    mpo_db: f64,
    gain_db_smoothed: f64,
    attack_ms: f64,
    release_ms: f64,
    attack_coef: f64,
    release_coef: f64,
    power_coef: f64,
    gain_coef: f64,
    mpo_release_coef: f64,
}

fn one_pole(tau_s: f64, fs: f64) -> f64 {
    1.0 - (-1.0 / (tau_s * fs)).exp()
}

impl BandCompressor {
    pub fn new(fs: f64) -> Self {
        Self {
            fs,
            power: 0.0,
            level_db: LEVEL_FLOOR_DB,
            mpo_db: LEVEL_FLOOR_DB,
            gain_db_smoothed: 0.0,
            // NaN never compares equal, so the first `process` call computes the coefficients.
            attack_ms: f64::NAN,
            release_ms: f64::NAN,
            attack_coef: 0.0,
            release_coef: 0.0,
            power_coef: one_pole(POWER_TAU_S, fs),
            gain_coef: one_pole(GAIN_TAU_S, fs),
            mpo_release_coef: one_pole(MPO_RELEASE_S, fs),
        }
    }

    /// Compresses `x` in place. Does not allocate. `ceiling_dbfs` caps the band output
    /// level (same dB scale as the detector); `f64::INFINITY` disables it.
    pub fn process(&mut self, x: &mut [f32], b: &BandFitting, attack_ms: f64, release_ms: f64, ceiling_dbfs: f64) {
        if attack_ms != self.attack_ms {
            self.attack_ms = attack_ms;
            self.attack_coef = one_pole(attack_ms / 1000.0, self.fs);
        }
        if release_ms != self.release_ms {
            self.release_ms = release_ms;
            self.release_coef = one_pole(release_ms / 1000.0, self.fs);
        }

        let mut power = self.power;
        let mut level = self.level_db;
        let mut mpo = self.mpo_db;
        let mut g = self.gain_db_smoothed;
        for s in x.iter_mut() {
            let p = (*s as f64) * (*s as f64);
            power += (p - power) * self.power_coef;
            let l = 10.0 * (2.0 * power + 1e-20).log10();
            let coef = if l > level { self.attack_coef } else { self.release_coef };
            level += (l - level) * coef;
            if l > mpo {
                mpo = l;
            } else {
                mpo += (l - mpo) * self.mpo_release_coef;
            }
            let target = gain_db(b, level).min(ceiling_dbfs - mpo);
            g += (target - g) * self.gain_coef;
            *s = ((*s as f64) * 10f64.powf(g / 20.0)) as f32;
        }
        self.power = power;
        self.level_db = level;
        self.mpo_db = mpo;
        self.gain_db_smoothed = g;
    }

    pub fn reset(&mut self) {
        self.power = 0.0;
        self.level_db = LEVEL_FLOOR_DB;
        self.mpo_db = LEVEL_FLOOR_DB;
        self.gain_db_smoothed = 0.0;
    }
}
