//! Auto level (Swift `AutoLevelMeasurement`): accumulates meter blocks and derives an output
//! gain. The processed level matches the input level, but never so high that the limiter has to
//! work: loudness beyond that belongs to the computer's volume, not to the output level.

use audiocurve_dsp::Meters;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AutoLevelResult {
    TooQuiet,
    Gain(f64),
}

#[derive(Clone, Debug, Default)]
pub struct AutoLevelMeasurement {
    in_power_sum: f64,
    out_power_sum: f64,
    sample_count: usize,
    /// Per block, the louder ear: output peak (dBFS) and safety-stage reduction (dB).
    peaks: Vec<f64>,
    reductions: Vec<f64>,
}

impl AutoLevelMeasurement {
    /// Peaks are kept this far under the ceiling.
    const MARGIN_DB: f64 = 1.0;
    /// Fraction of blocks allowed to exceed the statistics (short spikes are ignored).
    const QUANTILE: f64 = 0.9;

    /// Adds one meter block. Both channels count as one sample each.
    pub fn add(&mut self, m: &Meters) {
        for db in m.in_rms_db {
            self.in_power_sum += 10f64.powf(f64::from(db) / 10.0);
        }
        for db in m.out_rms_db {
            self.out_power_sum += 10f64.powf(f64::from(db) / 10.0);
        }
        self.sample_count += 2;
        self.peaks.push(f64::from(m.out_peak_db[0].max(m.out_peak_db[1])));
        self.reductions.push(f64::from(m.reduction_db[0].max(m.reduction_db[1]).max(0.0)));
    }

    /// Input below −50 dBFS gives `TooQuiet`. Otherwise the smaller of
    /// - loudness match: `current + in − out` (power means over all channels and blocks), and
    /// - limiter-idle: `current + (mpo − margin − peak90) − reduction90`, which lowers the gain
    ///   by what the limiter takes away and never lifts typical peaks into the ceiling,
    ///
    /// clamped to −48…12.
    pub fn result(&self, current_output_gain_db: f64, mpo_dbfs: f64) -> AutoLevelResult {
        if self.sample_count == 0 {
            return AutoLevelResult::TooQuiet;
        }
        let count = self.sample_count as f64;
        let in_db = 10.0 * (self.in_power_sum / count).log10();
        let out_db = 10.0 * (self.out_power_sum / count).log10();
        if in_db.is_nan() || in_db < -50.0 {
            return AutoLevelResult::TooQuiet;
        }
        let matched = current_output_gain_db + in_db - out_db;
        let idle = current_output_gain_db + (mpo_dbfs - Self::MARGIN_DB - percentile(&self.peaks))
            - percentile(&self.reductions);
        AutoLevelResult::Gain(matched.min(idle).clamp(-48.0, 12.0))
    }
}

fn percentile(v: &[f64]) -> f64 {
    let mut s: Vec<f64> = v.iter().copied().filter(|x| x.is_finite()).collect();
    if s.is_empty() {
        return 0.0;
    }
    s.sort_by(f64::total_cmp);
    s[((s.len() - 1) as f64 * AutoLevelMeasurement::QUANTILE).floor() as usize]
}
