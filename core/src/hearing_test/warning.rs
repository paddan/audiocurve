//! Warnings for the hearing test's result screen (Swift `TestWarning`).

use super::TestFlag;
use crate::profile::Ear;

/// The result's flags, with the per-frequency flags grouped into one warning per ear. The UI
/// turns each into localized text.
#[derive(Clone, Debug, PartialEq)]
pub enum TestWarning {
    RetestMismatch { ear: Ear, db: f64 },
    CatchTrialPresses,
    NoResponseAtMax { ear: Ear, frequencies_hz: Vec<f64> },
    NotConverged { ear: Ear, frequencies_hz: Vec<f64> },
    Asymmetry,
}

impl TestWarning {
    /// Fixed order: retest (right ear first), catch-trial presses, no response at max (right,
    /// left), not converged (right, left), asymmetry. Frequencies ascending, each once.
    pub fn from_flags(flags: &[TestFlag]) -> Vec<TestWarning> {
        const EARS: [Ear; 2] = [Ear::Right, Ear::Left];
        let mut retest: [Option<f64>; 2] = [None; 2];
        let mut catch_presses = false;
        let mut no_response: [Vec<f64>; 2] = Default::default();
        let mut not_converged: [Vec<f64>; 2] = Default::default();
        let mut asymmetry = false;
        let slot = |ear: Ear| EARS.iter().position(|&e| e == ear).unwrap_or(0);
        for flag in flags {
            match *flag {
                TestFlag::RetestMismatch { ear, db } => {
                    let r = &mut retest[slot(ear)];
                    *r = Some(r.map_or(db, |v| v.max(db)));
                }
                TestFlag::CatchTrialPresses(_) => catch_presses = true,
                TestFlag::NoResponseAtMax { ear, frequency_hz } => no_response[slot(ear)].push(frequency_hz),
                TestFlag::NotConverged { ear, frequency_hz } => not_converged[slot(ear)].push(frequency_hz),
                TestFlag::Asymmetry => asymmetry = true,
            }
        }
        let sorted = |v: &[f64]| {
            let mut v = v.to_vec();
            v.sort_by(f64::total_cmp);
            v.dedup();
            v
        };
        let mut out = Vec::new();
        for (i, &ear) in EARS.iter().enumerate() {
            if let Some(db) = retest[i] {
                out.push(TestWarning::RetestMismatch { ear, db });
            }
        }
        if catch_presses {
            out.push(TestWarning::CatchTrialPresses);
        }
        for (i, &ear) in EARS.iter().enumerate() {
            if !no_response[i].is_empty() {
                out.push(TestWarning::NoResponseAtMax { ear, frequencies_hz: sorted(&no_response[i]) });
            }
        }
        for (i, &ear) in EARS.iter().enumerate() {
            if !not_converged[i].is_empty() {
                out.push(TestWarning::NotConverged { ear, frequencies_hz: sorted(&not_converged[i]) });
            }
        }
        if asymmetry {
            out.push(TestWarning::Asymmetry);
        }
        out
    }
}
