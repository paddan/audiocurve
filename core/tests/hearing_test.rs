use audiocurve_core::hearing_test::{
    HearingTest, HearingTestResult, HearingTestStep, Presentation, TestCalibration, TestFlag, Transducer,
};
use audiocurve_core::profile::{Audiogram, Ear};

fn calibration() -> TestCalibration {
    TestCalibration::nominal(Transducer::InEar)
}

struct Run {
    presentations: Vec<Presentation>,
    result: HearingTestResult,
}

/// Drives a test to the end. `listener` sees each presentation and the number of presentations
/// so far (including this one) and says whether the tone was heard.
fn drive_with(
    cal: TestCalibration,
    random: impl FnMut() -> f64 + 'static,
    mut listener: impl FnMut(&Presentation, usize) -> bool,
) -> Run {
    let mut test = HearingTest::new(cal, Box::new(random));
    let mut seen = Vec::new();
    loop {
        match test.next() {
            HearingTestStep::Finished(result) => return Run { presentations: seen, result },
            HearingTestStep::Present(p) => {
                seen.push(p.clone());
                let heard = listener(&p, seen.len());
                test.respond(heard);
                assert!(seen.len() < 2000, "test did not terminate");
            }
        }
    }
}

fn drive(random: impl FnMut() -> f64 + 'static, listener: impl FnMut(&Presentation, usize) -> bool) -> Run {
    drive_with(calibration(), random, listener)
}

fn no_catch() -> f64 {
    0.5
}

/// A listener with a fixed threshold per (ear, frequency).
fn simulate(threshold_hl: impl Fn(Ear, f64) -> f64) -> Run {
    drive(no_catch, move |p, _| p.level_dbfs.is_some() && p.level_hl >= threshold_hl(p.ear, p.frequency_hz))
}

fn points(run: &Run, ear: Ear) -> &[audiocurve_core::hearing_test::TestPoint] {
    match ear {
        Ear::Left => &run.result.left,
        Ear::Right => &run.result.right,
    }
}

#[test]
fn finds_simulated_thresholds() {
    let run = simulate(|_, _| 35.0);
    for ear in Ear::ALL {
        assert_eq!(points(&run, ear).len(), 11);
        assert!(points(&run, ear).iter().all(|p| p.hl == 35.0));
    }
    assert!(run.result.flags.is_empty());
}

#[test]
fn floor_is_minus10() {
    let run = simulate(|_, _| -100.0);
    assert!(run.presentations.iter().all(|p| p.level_hl >= -10.0));
    assert!(run.result.left.iter().all(|p| p.hl == -10.0));
    assert!(run.result.right.iter().all(|p| p.hl == -10.0));
}

#[test]
fn never_exceeds_cap() {
    let run = simulate(|_, _| 1000.0);
    for p in &run.presentations {
        assert!(p.level_dbfs.unwrap() <= TestCalibration::cap_dbfs(p.frequency_hz));
    }
    for ear in Ear::ALL {
        for point in points(&run, ear) {
            assert_eq!(point.hl, TestCalibration::rounded_hl(calibration().max_hl(point.frequency_hz)));
            assert!(run.result.flags.contains(&TestFlag::NoResponseAtMax { ear, frequency_hz: point.frequency_hz }));
        }
    }
}

#[test]
fn familiarization_steps_above80() {
    let run = simulate(|_, _| 1000.0);
    let levels: Vec<f64> = run
        .presentations
        .iter()
        .take_while(|p| p.ear == Ear::Right && p.frequency_hz == 1000.0)
        .map(|p| p.level_hl)
        .collect();
    assert!(levels.starts_with(&[40.0, 60.0, 80.0, 85.0, 90.0, 95.0]), "{levels:?}");
}

#[test]
fn order_and_count() {
    let run = simulate(|_, _| 30.0);
    let mut sequence: Vec<(Ear, f64)> = Vec::new();
    let mut indices = Vec::new();
    for p in &run.presentations {
        let key = (p.ear, p.frequency_hz);
        if sequence.last() != Some(&key) {
            sequence.push(key);
            indices.push(p.index);
        }
    }
    let expected: Vec<(Ear, f64)> =
        [Ear::Right, Ear::Left].iter().flat_map(|&e| HearingTest::ORDER.iter().map(move |&f| (e, f))).collect();
    assert_eq!(sequence, expected);
    assert_eq!(indices, (1..=24).collect::<Vec<_>>());
    assert_eq!(run.result.left.len(), 11);
    let freqs: Vec<f64> = run.result.right.iter().map(|p| p.frequency_hz).collect();
    assert_eq!(freqs, Audiogram::STANDARD_FREQUENCIES);
}

#[test]
fn catch_trials_are_silent_and_counted() {
    // random() == 0 always asks for a catch trial, but never two in a row.
    let mut catch_seen = 0;
    let run = drive(
        || 0.0,
        |p, _| {
            if p.level_dbfs.is_none() {
                catch_seen += 1;
                return catch_seen <= 3;
            }
            p.level_hl >= 35.0
        },
    );
    assert!(catch_seen > 3);
    assert!(run.presentations.iter().any(|p| p.level_dbfs.is_none()));
    assert!(run.result.flags.contains(&TestFlag::CatchTrialPresses(3)));
    assert!(run.result.right.iter().all(|p| p.hl == 35.0));
}

#[test]
fn two_catch_presses_are_not_flagged() {
    let mut catch_seen = 0;
    let run = drive(
        || 0.0,
        |p, _| {
            if p.level_dbfs.is_none() {
                catch_seen += 1;
                return catch_seen <= 2;
            }
            p.level_hl >= 35.0
        },
    );
    assert!(!run.result.flags.iter().any(|f| matches!(f, TestFlag::CatchTrialPresses(_))));
}

#[test]
fn retest_mismatch_flagged() {
    let mut kilohertz_runs = 0;
    let mut last_frequency = 0.0;
    let run = drive(no_catch, |p, _| {
        if p.ear == Ear::Right && p.frequency_hz == 1000.0 && last_frequency != 1000.0 {
            kilohertz_runs += 1;
        }
        last_frequency = if p.ear == Ear::Right { p.frequency_hz } else { 0.0 };
        let threshold = if p.ear == Ear::Right && p.frequency_hz == 1000.0 && kilohertz_runs == 1 { 20.0 } else { 40.0 };
        p.level_hl >= threshold
    });
    assert_eq!(kilohertz_runs, 2);
    assert!(run.result.flags.contains(&TestFlag::RetestMismatch { ear: Ear::Right, db: 20.0 }));
    assert_eq!(run.result.right.iter().find(|p| p.frequency_hz == 1000.0).map(|p| p.hl), Some(20.0));
    assert!(!run.result.flags.contains(&TestFlag::RetestMismatch { ear: Ear::Left, db: 20.0 }));
}

#[test]
fn not_converged_bails_out() {
    // Heard only on descending presentations: no level ever gets 2 ascending hits.
    let mut previous: Option<Presentation> = None;
    let run = drive(no_catch, |p, _| {
        let heard = match &previous {
            Some(q) if q.ear == p.ear && q.frequency_hz == p.frequency_hz => p.level_hl < q.level_hl,
            _ => true,
        };
        previous = Some(p.clone());
        heard
    });
    let mut per_point = std::collections::BTreeMap::new();
    for p in &run.presentations {
        *per_point.entry((p.ear, p.index)).or_insert(0) += 1;
    }
    assert_eq!(per_point.len(), 24);
    assert!(per_point.values().all(|&n| n <= 12));
    for ear in Ear::ALL {
        for f in Audiogram::STANDARD_FREQUENCIES {
            assert!(run.result.flags.contains(&TestFlag::NotConverged { ear, frequency_hz: f }));
        }
    }
}

#[test]
fn asymmetry_flagged() {
    let run = simulate(|ear, _| if ear == Ear::Right { 10.0 } else { 60.0 });
    assert_eq!(run.result.flags.iter().filter(|f| **f == TestFlag::Asymmetry).count(), 1);
}

#[test]
fn extra_responses_ignored() {
    let mut test = HearingTest::new(calibration(), Box::new(no_catch));
    let HearingTestStep::Present(first) = test.next() else { panic!("no presentation") };
    test.respond(true);
    test.respond(true);
    let HearingTestStep::Present(second) = test.next() else { panic!("no presentation") };
    assert_eq!(first.level_hl, 40.0);
    assert_eq!(second.level_hl, 30.0);
}

#[test]
fn next_is_idempotent() {
    let mut test = HearingTest::new(calibration(), Box::new(no_catch));
    let a = test.next();
    let b = test.next();
    assert_eq!(a, b);
}

#[test]
fn never_exceeds_cap_with_low_full_scale_spl() {
    for transducer in [Transducer::InEar, Transducer::OverEar] {
        for full_scale_spl in [10.0, 20.0, 100.0] {
            let cal = TestCalibration { full_scale_spl, ..TestCalibration::nominal(transducer) };
            for hears_all in [true, false] {
                let run = drive_with(cal, no_catch, |p, _| hears_all && p.level_dbfs.is_some());
                for p in &run.presentations {
                    assert!(p.level_hl <= cal.max_hl(p.frequency_hz));
                    assert!(p.level_dbfs.unwrap() <= TestCalibration::cap_dbfs(p.frequency_hz));
                }
            }
        }
    }
}
