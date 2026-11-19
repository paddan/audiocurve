//! Prescription: per-band fittings from a hearing profile, and the kernel's parameter block
//! (Swift `FittingRule`).

use audiocurve_dsp::{BandFitting, Params};

pub use audiocurve_dsp::gain_db;

use crate::profile::{Audiogram, Ear, HearingProfile};
use crate::{BAND_COUNT, CENTERS};

/// `Auto`: the kernel glides between the Speech and Music fitting and time constants by its classifier.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CompressionMode {
    Speech,
    Music,
    Auto,
}

/// Which prescription a fitting set follows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Prescription {
    /// NAL-inspired fitting for intelligibility.
    Speech,
    /// Near-linear fitting for music: gentle compression, little treble boost.
    Music,
}

/// Threshold (dB HL) at `f`, log-frequency linear interpolation between measured points.
/// Held constant outside the measured range; 0 for an empty audiogram.
pub fn interpolated_hl(a: &Audiogram, f: f64) -> f64 {
    let pts = &a.points;
    let (Some(first), Some(last)) = (pts.first(), pts.last()) else { return 0.0 };
    if f <= first.frequency_hz {
        return first.threshold_db_hl;
    }
    if f >= last.frequency_hz {
        return last.threshold_db_hl;
    }
    for w in pts.windows(2) {
        let (lo, hi) = (w[0], w[1]);
        if f <= hi.frequency_hz {
            let t = (f.ln() - lo.frequency_hz.ln()) / (hi.frequency_hz.ln() - lo.frequency_hz.ln());
            return lo.threshold_db_hl + t * (hi.threshold_db_hl - lo.threshold_db_hl);
        }
    }
    last.threshold_db_hl
}

/// Per-band fitting for one ear. `strength` and `tone` are clamped to 0...1; NaN counts as 0.
pub fn bands(p: &HearingProfile, ear: Ear, strength: f64, tone: f64, rx: Prescription) -> [BandFitting; BAND_COUNT] {
    let s = clamp01(strength);
    let tn = clamp01(tone);
    let audiogram = p.audiogram(ear);
    let trim = p.trim(ear);
    let max_ratio = if rx == Prescription::Speech { 3.0 } else { 2.0 };
    std::array::from_fn(|i| {
        let f = CENTERS[i];
        let hl = interpolated_hl(audiogram, f);
        // Effective loss: above 60 dB HL only half of the extra loss is prescribed.
        let hle = if hl <= 60.0 { hl } else { 60.0 + 0.5 * (hl - 60.0) };
        let (soft, loud) = match rx {
            Prescription::Speech => {
                let corr = if f <= 250.0 {
                    10.0
                } else if f == 500.0 {
                    5.0
                } else {
                    0.0
                };
                (0.6 * hle - corr, 0.25 * hle - corr)
            }
            Prescription::Music => (0.45 * hle, if f <= 2000.0 { 0.3 } else { 0.2 } * hle),
        };
        // Tone: more bass and low mids, less loud-level treble.
        let boost = if f <= 250.0 {
            5.0
        } else if f == 500.0 {
            3.0
        } else if f == 750.0 || f == 1000.0 {
            1.0
        } else {
            0.0
        };
        // Added after the floor so the speech low-frequency correction cannot swallow it.
        let tone_soft = soft.max(0.0) + boost * tn;
        let tone_loud = (loud - if f > 2000.0 { 6.0 * tn } else { 0.0 }).max(0.0) + boost * tn;
        let t = trim.get(i).copied().unwrap_or(0.0);
        let g_soft = (tone_soft + t) * s;
        let g_loud = (tone_loud + t) * s;
        BandFitting { gain_soft_db: g_soft, gain_loud_db: g_loud, compression_ratio: ratio(g_soft, g_loud, max_ratio) }
    })
}

/// Compression ratio from the soft-to-loud gain difference over a 60 dB input range.
fn ratio(soft: f64, loud: f64, max_ratio: f64) -> f64 {
    let den = 60.0 - (soft - loud);
    if den <= 0.0 { max_ratio } else { (60.0 / den).max(1.0).min(max_ratio) }
}

/// Linear blend of two fittings for display (weight 1 = `speech`, 0 = `music`). The ratio is
/// recomputed from the blended gains and not clamped.
pub fn blend(
    speech: &[BandFitting; BAND_COUNT],
    music: &[BandFitting; BAND_COUNT],
    weight: f64,
) -> [BandFitting; BAND_COUNT] {
    let w = clamp01(weight);
    std::array::from_fn(|i| {
        let (s, m) = (speech[i], music[i]);
        let soft = w * s.gain_soft_db + (1.0 - w) * m.gain_soft_db;
        let loud = w * s.gain_loud_db + (1.0 - w) * m.gain_loud_db;
        let den = 60.0 - (soft - loud);
        BandFitting {
            gain_soft_db: soft,
            gain_loud_db: loud,
            compression_ratio: if den <= 0.0 { 10.0 } else { (60.0 / den).max(1.0) },
        }
    })
}

/// Everything `params` needs. `new` sets the defaults: tone 0, compression and limiter on,
/// input gain 0, active.
#[derive(Clone, Copy, Debug)]
pub struct ParamsInput<'a> {
    pub profile: Option<&'a HearingProfile>,
    pub strength: f64,
    pub tone: f64,
    pub mpo_dbfs: f64,
    pub output_gain_db: f64,
    pub mode: CompressionMode,
    /// false: every band gets a fixed gain (its loud-level gain, ratio 1:1).
    pub compression: bool,
    /// false: the band budget and the lookahead limiter are off in the kernel.
    pub limiter: bool,
    pub input_gain_db: f64,
    pub active: bool,
}

impl<'a> ParamsInput<'a> {
    pub fn new(
        profile: Option<&'a HearingProfile>,
        strength: f64,
        mpo_dbfs: f64,
        output_gain_db: f64,
        mode: CompressionMode,
    ) -> Self {
        Self {
            profile,
            strength,
            tone: 0.0,
            mpo_dbfs,
            output_gain_db,
            mode,
            compression: true,
            limiter: true,
            input_gain_db: 0.0,
            active: true,
        }
    }
}

/// Builds the kernel's parameter block. No profile gives unity fitting on all bands.
pub fn params(input: &ParamsInput) -> Params {
    let s = clamp01(input.strength);
    let set = |ear: Ear, rx: Prescription| -> [BandFitting; BAND_COUNT] {
        let b = match input.profile {
            Some(p) => bands(p, ear, s, input.tone, rx),
            None => [BandFitting::UNITY; BAND_COUNT],
        };
        if input.compression {
            b
        } else {
            b.map(|f| BandFitting { gain_soft_db: f.gain_loud_db, gain_loud_db: f.gain_loud_db, compression_ratio: 1.0 })
        }
    };
    Params {
        left: set(Ear::Left, Prescription::Speech),
        right: set(Ear::Right, Prescription::Speech),
        music_left: set(Ear::Left, Prescription::Music),
        music_right: set(Ear::Right, Prescription::Music),
        mpo_dbfs: clamp_or(input.mpo_dbfs, -20.0, -1.0, -3.0),
        output_gain_db: clamp_or(input.output_gain_db, -48.0, 12.0, 0.0),
        speech_weight: if input.mode == CompressionMode::Speech { 1.0 } else { 0.0 },
        limiter_enabled: if input.limiter { 1.0 } else { 0.0 },
        input_gain_db: clamp_or(input.input_gain_db, -30.0, 12.0, 0.0),
        auto_mode: if input.mode == CompressionMode::Auto { 1.0 } else { 0.0 },
        active: if input.active { 1.0 } else { 0.0 },
    }
}

fn clamp01(v: f64) -> f64 {
    clamp_or(v, 0.0, 1.0, 0.0)
}

/// `v` clamped to `lo..=hi`; NaN gives `nan_value`.
fn clamp_or(v: f64, lo: f64, hi: f64, nan_value: f64) -> f64 {
    if v.is_nan() { nan_value } else { v.clamp(lo, hi) }
}
