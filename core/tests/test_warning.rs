use audiocurve_core::hearing_test::{HearingTestRecord, TestFlag, TestWarning};
use audiocurve_core::profile::Ear;

#[test]
fn no_flags_give_no_warnings() {
    assert_eq!(TestWarning::from_flags(&[]), vec![]);
}

#[test]
fn per_frequency_flags_are_grouped_per_ear_right_first_sorted_by_frequency() {
    let flags = [
        TestFlag::NoResponseAtMax { ear: Ear::Right, frequency_hz: 8000.0 },
        TestFlag::NotConverged { ear: Ear::Left, frequency_hz: 3000.0 },
        TestFlag::NoResponseAtMax { ear: Ear::Left, frequency_hz: 6000.0 },
        TestFlag::NoResponseAtMax { ear: Ear::Right, frequency_hz: 125.0 },
        TestFlag::NotConverged { ear: Ear::Left, frequency_hz: 500.0 },
    ];
    assert_eq!(
        TestWarning::from_flags(&flags),
        vec![
            TestWarning::NoResponseAtMax { ear: Ear::Right, frequencies_hz: vec![125.0, 8000.0] },
            TestWarning::NoResponseAtMax { ear: Ear::Left, frequencies_hz: vec![6000.0] },
            TestWarning::NotConverged { ear: Ear::Left, frequencies_hz: vec![500.0, 3000.0] },
        ]
    );
}

#[test]
fn warnings_come_in_a_fixed_order() {
    let flags = [
        TestFlag::Asymmetry,
        TestFlag::NotConverged { ear: Ear::Right, frequency_hz: 1000.0 },
        TestFlag::NoResponseAtMax { ear: Ear::Left, frequency_hz: 8000.0 },
        TestFlag::CatchTrialPresses(4),
        TestFlag::RetestMismatch { ear: Ear::Left, db: 15.0 },
        TestFlag::RetestMismatch { ear: Ear::Right, db: 20.0 },
    ];
    assert_eq!(
        TestWarning::from_flags(&flags),
        vec![
            TestWarning::RetestMismatch { ear: Ear::Right, db: 20.0 },
            TestWarning::RetestMismatch { ear: Ear::Left, db: 15.0 },
            TestWarning::CatchTrialPresses,
            TestWarning::NoResponseAtMax { ear: Ear::Left, frequencies_hz: vec![8000.0] },
            TestWarning::NotConverged { ear: Ear::Right, frequencies_hz: vec![1000.0] },
            TestWarning::Asymmetry,
        ]
    );
}

#[test]
fn duplicate_frequency_flags_are_listed_once() {
    let flags = [
        TestFlag::NotConverged { ear: Ear::Right, frequency_hz: 1000.0 },
        TestFlag::NotConverged { ear: Ear::Right, frequency_hz: 1000.0 },
    ];
    assert_eq!(
        TestWarning::from_flags(&flags),
        vec![TestWarning::NotConverged { ear: Ear::Right, frequencies_hz: vec![1000.0] }]
    );
}

#[test]
fn default_result_name_is_device_and_date() {
    assert_eq!(HearingTestRecord::default_name("AirPods Pro", "16 Nov 2026"), "AirPods Pro – 16 Nov 2026");
}
