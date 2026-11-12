//! `repr(C)` parameter and meter types shared with Swift, plus the static gain curve.

use crate::bands::BAND_COUNT;

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BandFitting {
    pub gain_soft_db: f64,
    pub gain_loud_db: f64,
    pub compression_ratio: f64,
}

impl BandFitting {
    pub const UNITY: Self = Self { gain_soft_db: 0.0, gain_loud_db: 0.0, compression_ratio: 1.0 };
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Params {
    pub left: [BandFitting; BAND_COUNT],
    pub right: [BandFitting; BAND_COUNT],
    pub mpo_dbfs: f64,
    pub output_gain_db: f64,
    pub attack_ms: f64,
    pub release_ms: f64,
    /// 1.0 = band budget and lookahead limiter on; 0.0 = both off (output only
    /// hard-clamped at 0 dBFS). For listening experiments.
    pub limiter_enabled: f64,
    /// Gain (dB) before the bands: how hard the input drives the compressors. Applied before
    /// the In meter and spectrum, so those show what the bands get.
    pub input_gain_db: f64,
    /// 1.0 = Auto: attack/release glide between Musik (`attack_ms`/`release_ms`) and Tal
    /// (5/50 ms) following the speech/music classifier; 0.0 = use `attack_ms`/`release_ms`.
    pub auto_mode: f64,
}

impl Params {
    pub fn unity() -> Self {
        Self {
            left: [BandFitting::UNITY; BAND_COUNT],
            right: [BandFitting::UNITY; BAND_COUNT],
            mpo_dbfs: -3.0,
            output_gain_db: 0.0,
            attack_ms: 5.0,
            release_ms: 50.0,
            limiter_enabled: 1.0,
            input_gain_db: 0.0,
            auto_mode: 0.0,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Meters {
    pub in_rms_db: [f32; 2],
    pub in_peak_db: [f32; 2],
    pub out_rms_db: [f32; 2],
    pub out_peak_db: [f32; 2],
    /// How much the safety stage (band budget + limiter) currently lowers each ear (dB, >= 0).
    pub reduction_db: [f32; 2],
    /// Classifier output, 0 = music … 1 = speech (always computed; used in Auto mode).
    pub speech: f32,
}

/// Static gain curve (dB) for an input level in dBFS.
/// Below -75: expansion 1:2; -75..-60: constant `gain_soft_db`;
/// above -60: `gain_soft_db + (L + 60) * (1/CR - 1)`.
pub fn gain_db(b: &BandFitting, level_dbfs: f64) -> f64 {
    if level_dbfs < -75.0 {
        b.gain_soft_db - (-75.0 - level_dbfs)
    } else if level_dbfs <= -60.0 {
        b.gain_soft_db
    } else {
        b.gain_soft_db + (level_dbfs + 60.0) * (1.0 / b.compression_ratio - 1.0)
    }
}
