mod support;

use std::time::{Duration, Instant};

use audiocurve_app::i18n::{self, Lang};
use audiocurve_app::model::editor::Editor;
use audiocurve_core::hearing_test::{
    CalibrationKind, HearingTestRecord, TestCalibration, Transducer,
};
use audiocurve_core::outputs::OutputDevice;
use audiocurve_core::profile::HearingProfile;
use audiocurve_core::store::ProfileStore;
use support::example_profile;

fn store() -> (tempfile::TempDir, ProfileStore) {
    let dir = tempfile::tempdir().unwrap();
    let s = ProfileStore::new(dir.path().to_path_buf());
    (dir, s)
}

fn edited() -> HearingProfile {
    let mut p = example_profile();
    p.left_trim_db[3] = 4.0;
    p
}

fn record() -> HearingTestRecord {
    HearingTestRecord {
        date: std::time::SystemTime::now(),
        output: OutputDevice {
            uid: "uid".into(),
            name: "Headphones".into(),
        },
        calibration: TestCalibration {
            transducer: Transducer::InEar,
            full_scale_spl: 100.0,
            kind: CalibrationKind::Nominal,
        },
        left: vec![],
        right: vec![],
        flags: vec![],
        app_version: "test".into(),
    }
}

fn editor_with_profile(s: &ProfileStore) -> Editor {
    let mut e = Editor::new(s.clone());
    e.set_profile(example_profile(), Instant::now());
    e
}

#[test]
fn save_as_then_dirty_after_edit() {
    let (_d, s) = store();
    let mut e = editor_with_profile(&s);
    assert!(e.is_dirty());
    assert_eq!(e.current_name(), None);
    e.save_as("Mine");
    assert_eq!(e.current_name(), Some("Mine"));
    assert!(!e.is_dirty());
    assert!(e.error.is_none());
    e.set_profile(edited(), Instant::now());
    assert!(e.is_dirty());
}

#[test]
fn save_overwrites_source_and_clears_dirty() {
    let (_d, s) = store();
    let mut e = editor_with_profile(&s);
    e.save_as("Mine");
    let id = e.source_id.unwrap();
    e.set_profile(edited(), Instant::now());
    assert!(e.save());
    assert!(!e.is_dirty());
    assert_eq!(s.saved(id).unwrap().profile, edited());
}

#[test]
fn save_without_source_returns_false() {
    let (_d, s) = store();
    let mut e = editor_with_profile(&s);
    assert!(!e.save());
    assert!(s.list().is_empty());
}

#[test]
fn name_taken_sets_error_and_error_clears_after_3s() {
    let (_d, s) = store();
    let mut e = editor_with_profile(&s);
    e.save_as("Mine");
    let t0 = Instant::now();
    e.save_as("mine");
    assert_eq!(
        e.error.as_deref(),
        Some("A version with that name already exists")
    );
    // The first deadline is the autosave; once it has run, the error expiry is next.
    e.tick(e.next_deadline().unwrap());
    let expiry = e.next_deadline().unwrap();
    assert!(expiry >= t0 + Duration::from_secs(3), "error must last 3 s");
    assert!(
        expiry < t0 + Duration::from_secs(4),
        "error must not last 4 s"
    );
    e.tick(expiry - Duration::from_millis(1));
    assert!(e.error.is_some());
    e.tick(expiry);
    assert!(e.error.is_none());
    e.save_as("  ");
    assert_eq!(e.error.as_deref(), Some("Enter a name"));
}

#[test]
fn flush_writes_a_pending_autosave_immediately() {
    let (_d, s) = store();
    let mut e = Editor::new(s.clone());
    e.set_profile(example_profile(), Instant::now());
    assert!(s.load_working_copy().is_none());
    e.flush_autosave();
    assert_eq!(s.load_working_copy().unwrap().profile, example_profile());
    // Nothing is pending any more.
    assert!(e.next_deadline().is_none());
}

#[test]
fn duplicate_uses_translated_copy_name() {
    let (_d, s) = store();
    i18n::set_lang(Lang::En);
    let mut e = editor_with_profile(&s);
    e.save_as("X");
    let id = e.source_id.unwrap();
    e.duplicate(id);
    e.duplicate(id);
    let names: Vec<String> = e.saved.iter().map(|p| p.name.clone()).collect();
    assert!(names.contains(&"X copy".to_string()), "{names:?}");
    assert!(names.contains(&"X copy 2".to_string()), "{names:?}");
}

#[test]
fn delete_of_loaded_profile_keeps_profile_but_clears_source() {
    let (_d, s) = store();
    let mut e = editor_with_profile(&s);
    e.save_as("Mine");
    let id = e.source_id.unwrap();
    e.delete(id);
    assert_eq!(e.source_id, None);
    assert_eq!(e.profile, Some(example_profile()));
    assert!(e.saved.is_empty());
    assert!(e.is_dirty());
}

#[test]
fn autosave_is_written_after_half_a_second_and_restored_by_new() {
    let (_d, s) = store();
    let mut e = Editor::new(s.clone());
    assert!(e.profile.is_none());
    let t0 = Instant::now();
    e.set_profile(edited(), t0);
    assert_eq!(e.next_deadline(), Some(t0 + Duration::from_millis(500)));
    e.tick(t0 + Duration::from_millis(400));
    assert!(s.load_working_copy().is_none());
    e.tick(t0 + Duration::from_millis(500));
    assert_eq!(s.load_working_copy().unwrap().profile, edited());
    assert_eq!(e.next_deadline(), None);
    let restored = Editor::new(s);
    assert_eq!(restored.profile, Some(edited()));
}

#[test]
fn save_measured_picks_a_unique_name_and_does_not_load_it() {
    let (_d, s) = store();
    let mut e = editor_with_profile(&s);
    e.save_as("Test");
    let (source, profile) = (e.source_id, e.profile.clone());
    let id = uuid::Uuid::new_v4();
    let saved = e.save_measured(id, "Test", edited(), record()).unwrap();
    assert_eq!(saved.name, "Test (2)");
    assert_eq!(s.saved(id).unwrap().profile, edited());
    assert!(e.saved.iter().any(|p| p.id == id));
    assert_eq!(e.source_id, source);
    assert_eq!(e.profile, profile);
    assert_eq!(e.current_name(), Some("Test"));
}
