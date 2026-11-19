use audiocurve_core::outputs::{ActiveOutputs, OutputDevice};

fn dev(uid: &str, name: &str) -> OutputDevice {
    OutputDevice { uid: uid.into(), name: name.into() }
}
fn airpods() -> OutputDevice {
    dev("airpods-uid", "AirPods Pro")
}
fn aids() -> OutputDevice {
    dev("aids-uid", "Hearing aids")
}
fn speakers() -> OutputDevice {
    dev("spk", "MacBook Pro Speakers")
}
fn with(devices: Vec<OutputDevice>) -> ActiveOutputs {
    ActiveOutputs { devices }
}

#[test]
fn empty_selection_is_active_everywhere() {
    let a = ActiveOutputs::default();
    assert!(a.is_all());
    assert!(a.is_active(Some("anything")));
    assert!(a.is_active(None));
}

#[test]
fn only_selected_devices_are_active() {
    let a = with(vec![airpods()]);
    assert!(a.is_active(Some("airpods-uid")));
    assert!(!a.is_active(Some("aids-uid")));
    // An unknown current output never turns processing off.
    assert!(a.is_active(None));
}

#[test]
fn every_box_is_checked_when_all_are_active() {
    let a = ActiveOutputs::default();
    assert!(a.is_checked("airpods-uid"));
    assert!(a.is_checked("spk"));
}

#[test]
fn unchecking_from_all_keeps_the_others() {
    let mut a = ActiveOutputs::default();
    a.set_enabled(&aids(), false, &[airpods(), aids(), speakers()]);
    assert_eq!(a.devices, [airpods(), speakers()]);
    assert!(!a.is_checked("aids-uid"));
}

#[test]
fn checking_and_unchecking_single_devices() {
    let mut a = with(vec![airpods()]);
    a.set_enabled(&speakers(), true, &[airpods(), aids(), speakers()]);
    assert_eq!(a.devices, [airpods(), speakers()]);
    a.set_enabled(&speakers(), false, &[airpods(), aids(), speakers()]);
    assert_eq!(a.devices, [airpods()]);
}

#[test]
fn the_last_checked_box_cannot_be_unchecked() {
    let mut a = with(vec![airpods()]);
    assert!(a.is_last_checked("airpods-uid", &[airpods(), aids()]));
    a.set_enabled(&airpods(), false, &[airpods(), aids()]);
    assert_eq!(a.devices, [airpods()]);
}

#[test]
fn checking_every_device_means_all() {
    let mut a = with(vec![airpods()]);
    a.set_enabled(&aids(), true, &[airpods(), aids()]);
    assert!(a.is_all());
}

#[test]
fn menu_lists_available_then_remembered_offline() {
    let a = with(vec![airpods()]);
    assert_eq!(a.menu_devices(&[speakers(), aids()]), [speakers(), aids(), airpods()]);
    assert_eq!(a.menu_devices(&[airpods(), speakers()]), [airpods(), speakers()]);
}

#[test]
fn active_outputs_round_trip_and_defaults() {
    assert_eq!(ActiveOutputs::default(), with(vec![]));
    let a = with(vec![airpods()]);
    let json = serde_json::to_string(&a).unwrap();
    assert_eq!(serde_json::from_str::<ActiveOutputs>(&json).unwrap(), a);
    assert!(serde_json::from_str::<ActiveOutputs>("not json").is_err());
}
