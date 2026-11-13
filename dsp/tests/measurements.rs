use std::collections::BTreeMap;

#[path = "support/measure/mod.rs"]
mod measure;

#[test]
fn no_metric_worse_than_2_0_0() {
    let base: BTreeMap<String, f64> =
        serde_json::from_str(include_str!("../measurements/baseline-2.0.0.json")).unwrap();
    let now = measure::run_all(None);
    for (k, b) in &base {
        if !measure::report::LOWER_IS_BETTER.iter().any(|s| k.split('/').last().unwrap().starts_with(s)) {
            continue;
        }
        let tol = if k.contains("safety_active_pct") { 1.0 } else { 0.5 };
        assert!(now[k] <= b + tol, "{k}: {} > baseline {b}", now[k]);
    }
}

#[test]
fn metrics_are_finite_and_complete() {
    let now = measure::run_all(None);
    assert_eq!(now.len(), 2 * (2 * (9 + 4 + 1 + 3 + 1 + 2) + 3));
    assert!(now.values().all(|v| v.is_finite()));
}
