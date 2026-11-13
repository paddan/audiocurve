//! Fitting rule used by the measurements. In 2.0.0 this is the old pump-test rule.

use super::{support, Audiogram, Ear, Mode};
use audiocurve_dsp::BandFitting;

pub fn fittings(a: &Audiogram, ear: Ear, _mode: Mode) -> [BandFitting; 12] {
    let pts = match ear {
        Ear::Left => a.left,
        Ear::Right => a.right,
    };
    support::pump_fit(pts, 1.0)
}
