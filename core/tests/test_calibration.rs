use audiocurve_core::hearing_test::{TestCalibration, Transducer};
use audiocurve_core::profile::Audiogram;

const FREQS: [f64; 11] = Audiogram::STANDARD_FREQUENCIES;

#[test]
fn caps_per_frequency() {
    for f in [125.0, 250.0, 500.0] {
        assert_eq!(TestCalibration::cap_dbfs(f), -1.0);
    }
    for f in [750.0, 1000.0, 1500.0] {
        assert_eq!(TestCalibration::cap_dbfs(f), -3.0);
    }
    for f in [2000.0, 3000.0, 4000.0, 6000.0, 8000.0] {
        assert_eq!(TestCalibration::cap_dbfs(f), -5.0);
    }
}

#[test]
fn conversion_round_trips() {
    let c = TestCalibration::nominal(Transducer::InEar);
    for f in FREQS {
        assert!((c.dbfs_from_hl(c.hl_from_dbfs(-50.0, f), f) + 50.0).abs() < 1e-9);
        assert_eq!(c.hl_from_dbfs(-60.0, f), -60.0 + 100.0 - TestCalibration::retspl(Transducer::InEar, f));
    }
}

#[test]
fn max_hl_is_multiple_of5_and_under_cap() {
    for t in [Transducer::InEar, Transducer::OverEar] {
        let c = TestCalibration::nominal(t);
        for f in FREQS {
            assert_eq!(c.max_hl(f) % 5.0, 0.0);
            assert!(c.dbfs_from_hl(c.max_hl(f), f) <= TestCalibration::cap_dbfs(f) + 1e-9);
        }
    }
}

#[test]
fn retspl_shape() {
    for t in [Transducer::InEar, Transducer::OverEar] {
        assert!(TestCalibration::retspl(t, 125.0) > TestCalibration::retspl(t, 1000.0) + 10.0);
    }
    assert!(FREQS.iter().any(|&f| TestCalibration::retspl(Transducer::InEar, f) != TestCalibration::retspl(Transducer::OverEar, f)));
}

#[test]
fn rounding_and_clamp() {
    assert_eq!(TestCalibration::rounded_hl(42.4), 40.0);
    assert_eq!(TestCalibration::rounded_hl(42.6), 45.0);
    assert_eq!(TestCalibration::rounded_hl(-30.0), -10.0);
    assert_eq!(TestCalibration::rounded_hl(140.0), 120.0);
}

#[test]
fn default_transducer_from_name() {
    assert_eq!(TestCalibration::default_transducer("My AirPods Pro"), Transducer::InEar);
    assert_eq!(TestCalibration::default_transducer("WH-1000XM6"), Transducer::OverEar);
}
