#[path = "support/measure/mod.rs"]
mod measure;

use measure::prescription::{Rx, rule};

#[test]
fn prescription_cases_match_fixture() {
    let rows: Vec<serde_json::Value> = serde_json::from_str(include_str!(
        "../../Packages/AudioCurveCore/Tests/AudioCurveCoreTests/Fixtures/prescription-cases.json"
    ))
    .unwrap();
    assert!(rows.len() >= 12);
    for r in &rows {
        let n = |k: &str| r[k].as_f64().unwrap();
        let rx = if r["prescription"] == "speech" { Rx::Speech } else { Rx::Music };
        let fits = rule(&[(125.0, n("hl")), (8000.0, n("hl"))], rx, n("tone"), 1.0);
        let i = audiocurve_dsp::CENTERS.iter().position(|&c| c == n("f")).unwrap();
        let b = fits[i];
        assert!((b.gain_soft_db - n("soft")).abs() < 1e-3, "{r}: soft {}", b.gain_soft_db);
        assert!((b.gain_loud_db - n("loud")).abs() < 1e-3, "{r}: loud {}", b.gain_loud_db);
        assert!((b.compression_ratio - n("cr")).abs() < 1e-3, "{r}: cr {}", b.compression_ratio);
    }
}
