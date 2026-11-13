//! Per-band wide-dynamic-range compressor. The level-to-gain mapping is the static
//! curve `params::gain_db`; this module adds the level detector and gain smoothing.
//!
//! Detector: symmetric power average (5 ms), level in dB, then two attack/release
//! smoothers of the dB level. The slow one sets the gain, so syllables and transients
//! do not modulate it. When the fast one jumps more than `FAST_JUMP_DB` above it, the
//! slow level snaps up to the fast one and follows it while it rises, then returns with
//! the slow release. Applied gain is smoothed in dB with 20/3 ms.

use crate::params::{BandFitting, gain_db};

/// Power averaging time constant (symmetric rise and fall).
const POWER_TAU_S: f64 = 0.005;
/// Applied-gain smoothing time constant is 20/3 ms.
const GAIN_TAU_S: f64 = 0.020 / 3.0;
/// Initial level in dB; low enough that silence stays silent.
const LEVEL_FLOOR_DB: f64 = -120.0;
/// MPO detector: instantaneous band peak, held for `MPO_HOLD_S` so it does not ripple
/// with the waveform (a ripple there modulates the gain and is heard as distortion),
/// then released at a fixed rate. 25 ms covers half a period down to 20 Hz.
const MPO_HOLD_S: f64 = 0.025;
/// Release rate after the hold, dB per second.
const MPO_RELEASE_DB_PER_S: f64 = 60.0;
/// A new peak within this distance of the held level refreshes the hold.
const MPO_REFRESH_DB: f64 = 1.0;

/// Detector time constants in milliseconds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Timing {
    pub slow_attack_ms: f64,
    pub slow_release_ms: f64,
    pub fast_attack_ms: f64,
    pub fast_release_ms: f64,
}

/// Speech weight 0: slow, so music dynamics stay intact.
pub const MUSIC_TIMING: Timing =
    Timing { slow_attack_ms: 50.0, slow_release_ms: 1500.0, fast_attack_ms: 2.0, fast_release_ms: 50.0 };
/// Speech weight 1: still slow against syllables, but quick enough to follow phrases.
pub const SPEECH_TIMING: Timing =
    Timing { slow_attack_ms: 40.0, slow_release_ms: 400.0, fast_attack_ms: 2.0, fast_release_ms: 50.0 };
/// The fast level only overrides the slow one when it exceeds it by more than this.
pub const FAST_JUMP_DB: f64 = 8.0;

/// Geometric blend of every field: Music at 0, Speech at 1.
pub fn blend_timing(speech_weight: f64) -> Timing {
    let s = if speech_weight.is_nan() { 1.0 } else { speech_weight.clamp(0.0, 1.0) };
    let mix = |a: f64, b: f64| if s == 0.0 { a } else if s == 1.0 { b } else { a * (b / a).powf(s) };
    let (m, p) = (MUSIC_TIMING, SPEECH_TIMING);
    Timing {
        slow_attack_ms: mix(m.slow_attack_ms, p.slow_attack_ms),
        slow_release_ms: mix(m.slow_release_ms, p.slow_release_ms),
        fast_attack_ms: mix(m.fast_attack_ms, p.fast_attack_ms),
        fast_release_ms: mix(m.fast_release_ms, p.fast_release_ms),
    }
}

pub struct BandCompressor {
    fs: f64,
    power: f64,
    fast_db: f64,
    slow_db: f64,
    /// Set while the slow level follows a jump of the fast level.
    jump: bool,
    mpo_db: f64,
    mpo_hold_left: u32,
    mpo_hold: u32,
    mpo_release_step: f64,
    gain_db_smoothed: f64,
    timing: Timing,
    slow_attack_coef: f64,
    slow_release_coef: f64,
    fast_attack_coef: f64,
    fast_release_coef: f64,
    power_coef: f64,
    gain_coef: f64,
}

fn one_pole(tau_s: f64, fs: f64) -> f64 {
    1.0 - (-1.0 / (tau_s * fs)).exp()
}

impl BandCompressor {
    pub fn new(fs: f64) -> Self {
        Self {
            fs,
            power: 0.0,
            fast_db: LEVEL_FLOOR_DB,
            slow_db: LEVEL_FLOOR_DB,
            jump: false,
            mpo_db: LEVEL_FLOOR_DB,
            gain_db_smoothed: 0.0,
            // NaN never compares equal, so the first `analyze` call computes the coefficients.
            timing: Timing { slow_attack_ms: f64::NAN, slow_release_ms: f64::NAN, fast_attack_ms: f64::NAN, fast_release_ms: f64::NAN },
            slow_attack_coef: 0.0,
            slow_release_coef: 0.0,
            fast_attack_coef: 0.0,
            fast_release_coef: 0.0,
            power_coef: one_pole(POWER_TAU_S, fs),
            gain_coef: one_pole(GAIN_TAU_S, fs),
            mpo_hold_left: 0,
            mpo_hold: (MPO_HOLD_S * fs).round() as u32,
            mpo_release_step: MPO_RELEASE_DB_PER_S / fs,
        }
    }

    fn update_coefs(&mut self, t: &Timing) {
        let old = self.timing;
        let fs = self.fs;
        if t.slow_attack_ms != old.slow_attack_ms {
            self.slow_attack_coef = one_pole(t.slow_attack_ms / 1000.0, fs);
        }
        if t.slow_release_ms != old.slow_release_ms {
            self.slow_release_coef = one_pole(t.slow_release_ms / 1000.0, fs);
        }
        if t.fast_attack_ms != old.fast_attack_ms {
            self.fast_attack_coef = one_pole(t.fast_attack_ms / 1000.0, fs);
        }
        if t.fast_release_ms != old.fast_release_ms {
            self.fast_release_coef = one_pole(t.fast_release_ms / 1000.0, fs);
        }
        self.timing = *t;
    }

    /// Analysis pass: for every sample of `x` writes the smoothed WDRC gain (dB) to
    /// `gain_out` and the MPO detector level (dB, held band peak) to
    /// `level_out`. The static gain is `weight * speech + (1 - weight) * music`. Does not modify `x`. Does not allocate.
    ///
    /// Detector per sample: power level `l` (5 ms), fast level `lf` and slow level `ls`.
    /// When `lf > ls + FAST_JUMP_DB` the slow level snaps to the fast one and stays latched
    /// to it while `lf` keeps rising, so the gain follows a sudden loud sound down at the
    /// fast attack; once `lf` falls below `ls` the latch ends and `ls` returns with the
    /// slow attack/release. The static gain is taken from `ls`.
    pub fn analyze(&mut self, x: &[f32], speech: &BandFitting, music: &BandFitting, weight: f64,
                   timing: &Timing, gain_out: &mut [f32], level_out: &mut [f32]) {
        self.update_coefs(timing);
        let weight = weight.clamp(0.0, 1.0);
        let mut power = self.power;
        let mut fast = self.fast_db;
        let mut slow = self.slow_db;
        let mut jump = self.jump;
        let mut mpo = self.mpo_db;
        let mut hold = self.mpo_hold_left;
        let mut g = self.gain_db_smoothed;
        for ((s, go), lo) in x.iter().zip(gain_out.iter_mut()).zip(level_out.iter_mut()) {
            let p = (*s as f64) * (*s as f64);
            power += (p - power) * self.power_coef;
            let l = 10.0 * (2.0 * power + 1e-20).log10();
            fast += (l - fast) * if l > fast { self.fast_attack_coef } else { self.fast_release_coef };
            // A jump of the fast level above the slow one snaps the slow level up to it,
            // and the slow level keeps following the fast one while it rises.
            if fast > slow + FAST_JUMP_DB {
                jump = true;
            }
            if jump && fast >= slow {
                slow = fast;
            } else {
                jump = false;
                slow += (l - slow) * if l > slow { self.slow_attack_coef } else { self.slow_release_coef };
            }
            let level = slow;
            let pk = 20.0 * ((*s as f64).abs() + 1e-10).log10();
            if pk >= mpo - MPO_REFRESH_DB {
                mpo = mpo.max(pk);
                hold = self.mpo_hold;
            } else if hold > 0 {
                hold -= 1;
            } else {
                mpo = (mpo - self.mpo_release_step).max(pk);
            }
            // Evaluate only the fitting in use: 0 * inf would be NaN, and it halves the cost.
            let target = if weight == 1.0 {
                gain_db(speech, level)
            } else if weight == 0.0 {
                gain_db(music, level)
            } else {
                weight * gain_db(speech, level) + (1.0 - weight) * gain_db(music, level)
            };
            g += (target - g) * self.gain_coef;
            *go = g as f32;
            *lo = mpo as f32;
        }
        self.power = power;
        self.fast_db = fast;
        self.slow_db = slow;
        self.jump = jump;
        self.mpo_db = mpo;
        self.mpo_hold_left = hold;
        self.gain_db_smoothed = g;
    }

    /// Compresses `x` in place with a per-band output ceiling (`f64::INFINITY` disables
    /// it). Convenience wrapper over `analyze` for single-band use; processes in chunks
    /// on the stack. Does not allocate.
    pub fn process(&mut self, x: &mut [f32], b: &BandFitting, timing: &Timing, ceiling_dbfs: f64) {
        const CHUNK: usize = 256;
        let mut g = [0f32; CHUNK];
        let mut m = [0f32; CHUNK];
        for c in x.chunks_mut(CHUNK) {
            let n = c.len();
            self.analyze(c, b, b, 1.0, timing, &mut g[..n], &mut m[..n]);
            for ((s, &gi), &mi) in c.iter_mut().zip(&g[..n]).zip(&m[..n]) {
                let applied = (gi as f64).min(ceiling_dbfs - mi as f64);
                *s = ((*s as f64) * 10f64.powf(applied / 20.0)) as f32;
            }
        }
    }

    pub fn reset(&mut self) {
        self.power = 0.0;
        self.fast_db = LEVEL_FLOOR_DB;
        self.slow_db = LEVEL_FLOOR_DB;
        self.jump = false;
        self.mpo_db = LEVEL_FLOOR_DB;
        self.mpo_hold_left = 0;
        self.gain_db_smoothed = 0.0;
    }
}
