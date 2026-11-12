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
/// Power-averaging time constant of the MPO detector (faster than the WDRC detector so
/// onsets are caught before they reach the broadband limiter).
const MPO_POWER_TAU_S: f64 = 0.0005;

pub struct BandCompressor {
    fs: f64,
    power: f64,
    level_db: f64,
    mpo_db: f64,
    mpo_power: f64,
    mpo_power_coef: f64,
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
            mpo_power: 0.0,
            mpo_power_coef: one_pole(MPO_POWER_TAU_S, fs),
        }
    }

    fn update_coefs(&mut self, attack_ms: f64, release_ms: f64) {
        if attack_ms != self.attack_ms {
            self.attack_ms = attack_ms;
            self.attack_coef = one_pole(attack_ms / 1000.0, self.fs);
        }
        if release_ms != self.release_ms {
            self.release_ms = release_ms;
            self.release_coef = one_pole(release_ms / 1000.0, self.fs);
        }
    }

    /// Analysis pass: for every sample of `x` writes the smoothed WDRC gain (dB) to
    /// `gain_out` and the MPO detector level (dB, instant attack / 100 ms release) to
    /// `level_out`. Does not modify `x`. Does not allocate.
    pub fn analyze(&mut self, x: &[f32], b: &BandFitting, attack_ms: f64, release_ms: f64,
                   gain_out: &mut [f32], level_out: &mut [f32]) {
        self.update_coefs(attack_ms, release_ms);
        let mut power = self.power;
        let mut level = self.level_db;
        let mut mpo = self.mpo_db;
        let mut mp = self.mpo_power;
        let mut g = self.gain_db_smoothed;
        for ((s, go), lo) in x.iter().zip(gain_out.iter_mut()).zip(level_out.iter_mut()) {
            let p = (*s as f64) * (*s as f64);
            power += (p - power) * self.power_coef;
            let l = 10.0 * (2.0 * power + 1e-20).log10();
            let coef = if l > level { self.attack_coef } else { self.release_coef };
            level += (l - level) * coef;
            mp += (p - mp) * self.mpo_power_coef;
            let lm = 10.0 * (2.0 * mp + 1e-20).log10();
            if lm > mpo {
                mpo = lm;
            } else {
                mpo += (lm - mpo) * self.mpo_release_coef;
            }
            g += (gain_db(b, level) - g) * self.gain_coef;
            *go = g as f32;
            *lo = mpo as f32;
        }
        self.power = power;
        self.level_db = level;
        self.mpo_db = mpo;
        self.mpo_power = mp;
        self.gain_db_smoothed = g;
    }

    /// Compresses `x` in place with a per-band output ceiling (`f64::INFINITY` disables
    /// it). Convenience wrapper over `analyze` for single-band use; processes in chunks
    /// on the stack. Does not allocate.
    pub fn process(&mut self, x: &mut [f32], b: &BandFitting, attack_ms: f64, release_ms: f64, ceiling_dbfs: f64) {
        const CHUNK: usize = 256;
        let mut g = [0f32; CHUNK];
        let mut m = [0f32; CHUNK];
        for c in x.chunks_mut(CHUNK) {
            let n = c.len();
            self.analyze(c, b, attack_ms, release_ms, &mut g[..n], &mut m[..n]);
            for ((s, &gi), &mi) in c.iter_mut().zip(&g[..n]).zip(&m[..n]) {
                let applied = (gi as f64).min(ceiling_dbfs - mi as f64);
                *s = ((*s as f64) * 10f64.powf(applied / 20.0)) as f32;
            }
        }
    }

    pub fn reset(&mut self) {
        self.power = 0.0;
        self.level_db = LEVEL_FLOOR_DB;
        self.mpo_db = LEVEL_FLOOR_DB;
        self.mpo_power = 0.0;
        self.gain_db_smoothed = 0.0;
    }
}
