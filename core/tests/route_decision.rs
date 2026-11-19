use audiocurve_core::outputs::{ActiveOutputs, OutputDevice};
use audiocurve_core::route::{PermissionState as P, PluginLoadState as L, RouteFailure, RouteInputs, RouteTarget as T, route_target};

struct I {
    enabled: bool,
    permission: P,
    plugin: L,
    output: Option<&'static str>,
    active: ActiveOutputs,
    testing: bool,
}

impl Default for I {
    fn default() -> Self {
        I { enabled: true, permission: P::Granted, plugin: L::Loaded, output: Some("airpods"), active: ActiveOutputs::default(), testing: false }
    }
}

fn target(i: I) -> T {
    route_target(&RouteInputs {
        enabled: i.enabled,
        permission: i.permission,
        plugin: i.plugin,
        output_uid: i.output.map(String::from),
        active_outputs: i.active,
        testing: i.testing,
    })
}

fn running(uid: &str) -> T {
    T::Running { device_uid: uid.into() }
}
fn paused(uid: &str) -> T {
    T::Paused { device_uid: uid.into() }
}

#[test]
fn off_wins_over_everything() {
    assert_eq!(target(I { enabled: false, permission: P::Denied, plugin: L::Failed, ..I::default() }), T::Off);
}

#[test]
fn denied_permission_fails() {
    assert_eq!(target(I { permission: P::Denied, ..I::default() }), T::Failed(RouteFailure::Permission));
}

#[test]
fn unknown_permission_still_runs() {
    assert_eq!(target(I { permission: P::Unknown, ..I::default() }), running("airpods"));
}

#[test]
fn plugin_states() {
    assert_eq!(target(I { plugin: L::Loading, ..I::default() }), T::Starting);
    assert_eq!(target(I { plugin: L::Failed, ..I::default() }), T::Failed(RouteFailure::Plugin));
}

#[test]
fn unknown_output_fails_route() {
    assert_eq!(target(I { output: None, ..I::default() }), T::Failed(RouteFailure::Route));
}

#[test]
fn unticked_output_pauses() {
    let a = || ActiveOutputs { devices: vec![OutputDevice { uid: "airpods".into(), name: "AirPods Pro".into() }] };
    assert_eq!(target(I { output: Some("aids"), active: a(), ..I::default() }), paused("aids"));
    assert_eq!(target(I { output: Some("airpods"), active: a(), ..I::default() }), running("airpods"));
}

#[test]
fn empty_selection_runs_everywhere() {
    assert_eq!(target(I { output: Some("spk"), ..I::default() }), running("spk"));
}

#[test]
fn testing_pauses_route() {
    assert_eq!(target(I { testing: true, ..I::default() }), paused("airpods"));
    assert_eq!(target(I { output: None, testing: true, ..I::default() }), T::Off);
}

#[test]
fn testing_wins_over_plugin_state() {
    assert_eq!(target(I { plugin: L::Loading, testing: true, ..I::default() }), paused("airpods"));
}

#[test]
fn off_still_wins() {
    assert_eq!(target(I { enabled: false, testing: true, ..I::default() }), T::Off);
}
