mod support;

use audiocurve_core::profile::{Audiogram, Ear, HearingProfile, ProfileValidationError as E};
use support::points;

#[test]
fn flat_is_valid() {
    let p = HearingProfile::flat();
    assert_eq!(p.validate(), Ok(()));
    assert!(p.left.points.len() == 11 && p.right.points.len() == 11);
}

#[test]
fn too_few_points() {
    let mut p = HearingProfile::flat();
    p.left = Audiogram { points: p.left.points[..3].to_vec() };
    assert_eq!(p.validate(), Err(E::TooFewPoints(Ear::Left)));
}

#[test]
fn unsorted_or_duplicate() {
    let mut p = HearingProfile::flat();
    p.left = points(&[1000.0, 500.0, 2000.0, 4000.0], 0.0);
    assert_eq!(p.validate(), Err(E::UnsortedOrDuplicateFrequencies(Ear::Left)));
}

#[test]
fn non_standard_frequency() {
    let mut p = HearingProfile::flat();
    p.left = points(&[500.0, 900.0, 1000.0, 2000.0], 0.0);
    assert_eq!(p.validate(), Err(E::NonStandardFrequency(Ear::Left, 900.0)));
}

#[test]
fn threshold_bounds() {
    let profile = |hl: f64| {
        let mut p = HearingProfile::flat();
        p.left = points(&[125.0, 250.0, 500.0, 1000.0], hl);
        p
    };
    assert_eq!(profile(120.0).validate(), Ok(()));
    assert_eq!(profile(120.5).validate(), Err(E::ThresholdOutOfRange { ear: Ear::Left, frequency_hz: 125.0 }));
    assert_eq!(profile(-10.0).validate(), Ok(()));
    assert_eq!(profile(-10.5).validate(), Err(E::ThresholdOutOfRange { ear: Ear::Left, frequency_hz: 125.0 }));
}

#[test]
fn trim_bounds() {
    let mut p = HearingProfile::flat();
    p.left_trim_db[3] = 20.0;
    assert_eq!(p.validate(), Ok(()));
    p.left_trim_db[3] = 20.5;
    assert_eq!(p.validate(), Err(E::TrimOutOfRange { ear: Ear::Left, band: 3 }));
    let mut q = HearingProfile::flat();
    q.left_trim_db = vec![0.0; 11];
    assert_eq!(q.validate(), Err(E::WrongTrimCount(Ear::Left)));
}

#[test]
fn audiogram_accessors() {
    let mut p = HearingProfile::flat();
    let a = points(&[125.0, 250.0, 500.0, 1000.0], 30.0);
    p.set_audiogram(Ear::Right, a.clone());
    p.set_trim(Ear::Right, (1..=12).map(f64::from).collect());
    assert_eq!(p.audiogram(Ear::Right), &a);
    assert_eq!(p.trim(Ear::Right), (1..=12).map(f64::from).collect::<Vec<_>>().as_slice());
    assert_eq!(p.trim(Ear::Left), [0.0; 12].as_slice());
}

#[test]
fn bands() {
    assert_eq!(audiocurve_core::CENTERS.len(), 12);
    assert_eq!(audiocurve_dsp::crossovers().len(), 11);
}

#[test]
fn nan_threshold_and_trim_are_rejected() {
    let mut p = HearingProfile::flat();
    p.left.points[3].threshold_db_hl = f64::NAN;
    assert!(matches!(p.validate(), Err(E::ThresholdOutOfRange { ear: Ear::Left, .. })));
    let mut p = HearingProfile::flat();
    p.right_trim_db[5] = f64::NAN;
    assert_eq!(p.validate(), Err(E::TrimOutOfRange { ear: Ear::Right, band: 5 }));
}
