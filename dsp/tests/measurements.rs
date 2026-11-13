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
        if !measure::report::LOWER_IS_BETTER.iter().any(|s| k.split('/').last().unwrap().starts_with(s)) {
            continue;
        }
        let tol = if k.contains("safety_active_pct") { 1.0 } else { 0.5 };
        assert!(now[k] <= b + tol, "{k}: {} > baseline {b}", now[k]);
    }
}

#[test]
fn metrics_are_finite_and_complete() {
    let now = now();
    assert_eq!(now.len(), 2 * (2 * (9 + 4 + 1 + 3 + 1 + 2) + 3));
    assert!(now.values().all(|v| v.is_finite()));
}
