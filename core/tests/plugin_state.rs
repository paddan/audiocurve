mod support;

use audiocurve_core::plugin_state::PluginState;
use audiocurve_core::profile::{Audiogram, HearingProfile};
use support::{example_profile, points};

#[test]
fn round_trip() {
    let state = PluginState { version: PluginState::CURRENT_VERSION, profile: Some(example_profile()) };
    let decoded = PluginState::decode(Some(&state.encode()));
    assert_eq!(decoded, state);
    assert!(decoded.profile.is_some());
}

#[test]
fn missing_data_yields_nil_profile() {
    assert!(PluginState::decode(None).profile.is_none());
}

#[test]
fn corrupt_data_yields_nil_profile() {
    assert!(PluginState::decode(Some(b"{")).profile.is_none());
}

#[test]
fn unknown_version_yields_nil_profile() {
    let state = PluginState { version: 99, profile: Some(example_profile()) };
    assert!(PluginState::decode(Some(&state.encode())).profile.is_none());
}

#[test]
fn invalid_profile_yields_nil() {
    let mut p = HearingProfile::flat();
    p.left = points(&[125.0, 250.0, 500.0], 0.0);
    let _: &Audiogram = &p.left;
    let state = PluginState { version: PluginState::CURRENT_VERSION, profile: Some(p) };
    assert!(PluginState::decode(Some(&state.encode())).profile.is_none());
}

#[test]
fn resolve_keeps_current_when_host_has_no_profile() {
    let current = example_profile();
    assert_eq!(PluginState::resolve(None, Some(current.clone())), Some(current));
    assert_eq!(PluginState::resolve(None, None), None);
}

#[test]
fn resolve_prefers_host_profile() {
    let host = example_profile();
    assert_eq!(PluginState::resolve(Some(host.clone()), Some(HearingProfile::flat())), Some(host.clone()));
    assert_eq!(PluginState::resolve(Some(host.clone()), None), Some(host));
}
