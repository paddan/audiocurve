use std::collections::BTreeMap;
use std::sync::OnceLock;

#[path = "support/measure/mod.rs"]
mod measure;

fn now() -> &'static BTreeMap<String, f64> {
    static NOW: OnceLock<BTreeMap<String, f64>> = OnceLock::new();
    NOW.get_or_init(|| measure::run_all(None))
}

#[test]
fn no_metric_worse_than_2_0_0() {
    let base: BTreeMap<String, f64> =
        serde_json::from_str(include_str!("../measurements/baseline-2.0.0.json")).unwrap();
    let now = now();
    for (k, b) in &base {
        let name = k.split('/').last().unwrap();
        if measure::report::REPORT_ONLY.contains(&name) {
            continue;
        }
        if !measure::report::LOWER_IS_BETTER.iter().any(|s| k.split('/').last().unwrap().starts_with(s)) {
            continue;
        }
        // Distortion below -100 dB is inaudible, below the 16-bit noise floor (~-96 dB): count it as equal.
        let (b, v) = if name.starts_with("thdn_") || name.starts_with("imd_") {
            (b.max(-100.0), now[k].max(-100.0))
        } else {
            (*b, now[k])
        };
        let tol = if k.contains("safety_active_pct") { 1.0 } else { 0.5 };
        assert!(v <= b + tol, "{k}: {v} > baseline {b}");
    }
}

#[test]
fn gain_modulation_at_most_half_of_2_0_0() {
    let base: BTreeMap<String, f64> =
        serde_json::from_str(include_str!("../measurements/baseline-2.0.0.json")).unwrap();
    let now = now();
    let mut checked = 0;
    for (k, b) in base.iter().filter(|(k, _)| k.ends_with("/gainmod_hf")) {
        assert!(now[k] <= 0.5 * b, "{k}: {} > 0.5 x baseline {b}", now[k]);
        checked += 1;
    }
    assert_eq!(checked, 4);
}

#[test]
fn no_inter_sample_peak_over_ceiling() {
    // Spec 1: no 4x-oversampled inter-sample peak exceeds the MPO ceiling.
    let isp: Vec<_> = now().iter().filter(|(k, _)| k.ends_with("/isp_over_db")).collect();
    assert_eq!(isp.len(), 4);
    for (k, v) in isp {
        assert!(*v <= 0.0, "{k}: {v} dB over the ceiling");
    }
}

#[test]
fn metrics_are_finite_and_complete() {
    let now = now();
    // Per audiogram: speech and music each have 9 thdn + 4 imd + 1 gainmod + 4 safety (incl.
    // output gain) + 1 isp + 2 tilt; auto has the 4 safety keys only.
    assert_eq!(now.len(), 2 * (2 * (9 + 4 + 1 + 4 + 1 + 2) + 4));
    assert!(now.values().all(|v| v.is_finite()));
}
