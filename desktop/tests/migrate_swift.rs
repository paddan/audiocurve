use audiocurve_app::migrate_swift::{SwiftDefaults, migrate, parse_pending_volumes};
use audiocurve_app::model::settings::Settings;
use audiocurve_core::store::ProfileStore;
use audiocurve_core::volume::VolumeSnapshot;

#[test]
fn parses_swift_array_form() {
    let json = br#"[{"deviceUID":"x","scalars":[0,0.5,1,0.25],"failedAttempts":2}]"#;
    let all = parse_pending_volumes(json);
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].device_uid, "x");
    assert_eq!(all[0].scalars.get(&0), Some(&0.5));
    assert_eq!(all[0].scalars.get(&1), Some(&0.25));
    assert_eq!(all[0].failed_attempts, 2);
}

#[test]
fn parses_legacy_single_object() {
    let all = parse_pending_volumes(br#"{"deviceUID":"y","scalars":[2,0.75]}"#);
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].device_uid, "y");
    assert_eq!(all[0].scalars.get(&2), Some(&0.75));
}

#[test]
fn corrupt_json_gives_nothing() {
    assert!(parse_pending_volumes(b"not json").is_empty());
    assert!(parse_pending_volumes(b"42").is_empty());
    assert!(parse_pending_volumes(br#"[{"deviceUID":"x","scalars":[0]}]"#).is_empty());
    assert!(parse_pending_volumes(br#"{"deviceUID":"x"}"#).is_empty());
}

#[test]
fn snapshot_without_attempts_parses_with_zero() {
    let all = parse_pending_volumes(br#"[{"deviceUID":"x","scalars":[0,1]}]"#);
    assert_eq!(all[0].failed_attempts, 0);
}

#[test]
fn migrate_keeps_existing_original_and_fills_settings_once() {
    let dir = tempfile::tempdir().unwrap();
    let store = ProfileStore::new(dir.path().to_path_buf());
    store.save_pending_volume(VolumeSnapshot {
        device_uid: "x".into(),
        scalars: [(0, 0.3)].into(),
        failed_attempts: 0,
    });
    let d = SwiftDefaults {
        enabled: Some(false),
        background_processing: Some(false),
        show_in_menu_bar: None,
        asked_launch_at_login: None,
        pending_volume: Some(
            br#"[{"deviceUID":"x","scalars":[0,0.9,1,0.8]},{"deviceUID":"z","scalars":[0,0.6]}]"#
                .to_vec(),
        ),
    };
    assert!(migrate(&d, &store, dir.path()));
    let all = store.pending_volumes();
    let x = all.iter().find(|s| s.device_uid == "x").unwrap();
    assert_eq!(x.scalars.get(&0), Some(&0.3), "the original is kept");
    assert_eq!(x.scalars.get(&1), Some(&0.8), "a missing element is added");
    assert!(all.iter().any(|s| s.device_uid == "z"));
    let s = Settings::load(dir.path());
    assert!(!s.enabled && !s.background_processing && s.show_in_menu_bar);

    // Settings are filled only once: an existing settings.json is left alone.
    let other = SwiftDefaults {
        enabled: Some(true),
        background_processing: Some(true),
        ..Default::default()
    };
    assert!(
        !migrate(&other, &store, dir.path()),
        "no pending key to remove"
    );
    assert!(!Settings::load(dir.path()).enabled);
}

#[test]
fn unwritable_store_keeps_the_key() {
    let dir = tempfile::tempdir().unwrap();
    // A file where the store directory should be: nothing can be written under it.
    let root = dir.path().join("store");
    std::fs::write(&root, b"x").unwrap();
    let store = ProfileStore::new(root.clone());
    let d = SwiftDefaults {
        pending_volume: Some(br#"[{"deviceUID":"x","scalars":[0,0.5]}]"#.to_vec()),
        ..Default::default()
    };
    assert!(!migrate(&d, &store, &root), "the key must stay for a retry");
    assert!(store.pending_volumes().is_empty());
}

/// Swift's `askedLaunchAtLogin`: a user it already asked is not asked again.
#[test]
fn migrate_carries_the_launch_at_login_question_once() {
    let dir = tempfile::tempdir().unwrap();
    let store = ProfileStore::new(dir.path().to_path_buf());
    let d = SwiftDefaults {
        asked_launch_at_login: Some(true),
        ..Default::default()
    };
    migrate(&d, &store, dir.path());
    let s = Settings::load(dir.path());
    assert!(s.asked_launch_at_login);
    assert!(s.enabled && s.background_processing && s.show_in_menu_bar);

    // Write-once, like the other settings.
    let other = SwiftDefaults {
        asked_launch_at_login: Some(false),
        ..Default::default()
    };
    migrate(&other, &store, dir.path());
    assert!(Settings::load(dir.path()).asked_launch_at_login);
}
