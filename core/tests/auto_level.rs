use audiocurve_core::auto_level::{AutoLevelMeasurement, AutoLevelResult};
use audiocurve_dsp::Meters;

/// One meter block with the same value on both channels (peak 10 dB above RMS by default).
fn meters(in_db: f32, out_db: f32, peak_db: Option<f32>, reduction_db: f32) -> Meters {
    let pk = peak_db.unwrap_or(out_db + 10.0);
    Meters {
        in_rms_db: [in_db; 2],
        out_rms_db: [out_db; 2],
        out_peak_db: [pk; 2],
        reduction_db: [reduction_db; 2],
        ..Meters::default()
    }
}

fn gain(m: &AutoLevelMeasurement) -> f64 {
    match m.result(0.0, -3.0) {
        AutoLevelResult::Gain(g) => g,
        r => panic!("expected a gain result, got {r:?}"),
    }
}

#[test]
fn auto_level_matches() {
    let mut m = AutoLevelMeasurement::default();
    for _ in 0..60 {
        m.add(&meters(-20.0, -14.0, None, 0.0));
    }
    assert!((gain(&m) - -6.0).abs() < 0.01);
}

#[test]
fn auto_level_clamps() {
    let mut m = AutoLevelMeasurement::default();
    for _ in 0..60 {
        m.add(&meters(-20.0, -80.0, None, 0.0));
    }
    assert_eq!(m.result(0.0, -3.0), AutoLevelResult::Gain(12.0));
}

#[test]
fn auto_level_too_quiet() {
    let mut m = AutoLevelMeasurement::default();
    for _ in 0..60 {
        m.add(&meters(-60.0, -60.0, None, 0.0));
    }
    assert_eq!(m.result(0.0, -3.0), AutoLevelResult::TooQuiet);
}

#[test]
fn auto_level_no_samples_is_too_quiet() {
    assert_eq!(AutoLevelMeasurement::default().result(0.0, -3.0), AutoLevelResult::TooQuiet);
}

#[test]
fn auto_level_power_mean() {
    let mut m = AutoLevelMeasurement::default();
    for _ in 0..30 {
        m.add(&meters(-20.0, -20.0, None, 0.0));
    }
    for _ in 0..30 {
        m.add(&meters(-30.0, -30.0, None, 0.0));
    }
    assert!(gain(&m).abs() < 0.01);
}

#[test]
fn auto_level_backs_off_when_limiter_works() {
    let mut m = AutoLevelMeasurement::default();
    for _ in 0..60 {
        m.add(&meters(-20.0, -14.0, Some(-3.0), 10.0));
    }
    let g = gain(&m);
    assert!((g - -11.0).abs() < 0.01, "{g}");
}

#[test]
fn auto_level_never_raises_peaks_into_tak() {
    let mut m = AutoLevelMeasurement::default();
    for _ in 0..60 {
        m.add(&meters(-20.0, -26.0, Some(-5.0), 0.0));
    }
    let g = gain(&m);
    assert!((g - 1.0).abs() < 0.01, "{g}");
}

#[test]
fn auto_level_ignores_rare_reduction_spikes() {
    let mut m = AutoLevelMeasurement::default();
    for i in 0..60 {
        m.add(&meters(-20.0, -20.0, Some(-10.0), if i < 4 { 8.0 } else { 0.0 }));
    }
    let g = gain(&m);
    assert!(g.abs() < 0.01, "{g}");
}
