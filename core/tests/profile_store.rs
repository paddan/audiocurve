mod support;

use std::path::{Path, PathBuf};
use std::time::{Duration, UNIX_EPOCH};

use audiocurve_core::hearing_test::{HearingTestRecord, TestCalibration, TestFlag, TestPoint, Transducer};
use audiocurve_core::outputs::{ActiveOutputs, LevelSettings, OutputAssignments, OutputDevice};
use audiocurve_core::profile::HearingProfile;
use audiocurve_core::store::types::WorkingCopy;
use audiocurve_core::store::{ProfileStore, StoreError};
use support::{example_profile, temp_store};
use uuid::Uuid;

fn profile_file(dir: &Path, id: Uuid) -> PathBuf {
    dir.join("profiles").join(format!("{id}.json"))
}

fn flat() -> HearingProfile {
    HearingProfile::flat()
}

#[test]
fn save_new_and_list_round_trip() {
    let (_dir, store) = temp_store();
    let saved = store.save_new("  Home  ", &example_profile()).unwrap();
    assert_eq!(saved.name, "Home");
    let list = store.list();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].id, saved.id);
    assert_eq!(list[0].profile, example_profile());
    assert_eq!(store.saved(saved.id).unwrap().name, "Home");
    assert!(store.saved(Uuid::new_v4()).is_none());
}

#[test]
fn list_is_sorted_by_name() {
    let (_dir, store) = temp_store();
    for n in ["Zeta", "alfa", "Beta 10", "Beta 2"] {
        store.save_new(n, &flat()).unwrap();
    }
    let names: Vec<String> = store.list().into_iter().map(|s| s.name).collect();
    assert_eq!(names, ["alfa", "Beta 2", "Beta 10", "Zeta"]);
}

#[test]
fn duplicate_and_empty_names_rejected() {
    let (_dir, store) = temp_store();
    store.save_new("Home", &flat()).unwrap();
    assert_eq!(store.save_new(" home ", &flat()).unwrap_err(), StoreError::NameTaken);
    assert_eq!(store.save_new("   ", &flat()).unwrap_err(), StoreError::EmptyName);
    assert_eq!(store.list().len(), 1);
}

#[test]
fn long_names_are_truncated() {
    let (_dir, store) = temp_store();
    let saved = store.save_new(&"a".repeat(100), &flat()).unwrap();
    assert_eq!(saved.name.chars().count(), 60);
}

#[test]
fn invalid_profile_rejected() {
    let (_dir, store) = temp_store();
    let mut bad = flat();
    bad.left_trim_db = vec![];
    assert_eq!(store.save_new("X", &bad).unwrap_err(), StoreError::Io("invalid profile".into()));
    let ok = store.save_new("X", &flat()).unwrap();
    assert_eq!(store.overwrite(ok.id, &bad).unwrap_err(), StoreError::Io("invalid profile".into()));
}

#[test]
fn overwrite_replaces_profile_and_updates_modified() {
    let (_dir, store) = temp_store();
    let saved = store.save_new("X", &flat()).unwrap();
    store.overwrite(saved.id, &example_profile()).unwrap();
    let again = store.saved(saved.id).unwrap();
    assert_eq!(again.profile, example_profile());
    assert!(again.modified >= saved.modified);
    assert_eq!(again.name, "X");
    assert_eq!(store.overwrite(Uuid::new_v4(), &flat()).unwrap_err(), StoreError::NotFound);
}

#[test]
fn rename_including_to_own_name() {
    let (_dir, store) = temp_store();
    let a = store.save_new("A", &flat()).unwrap();
    store.save_new("B", &flat()).unwrap();
    store.rename(a.id, "a").unwrap();
    assert_eq!(store.saved(a.id).unwrap().name, "a");
    store.rename(a.id, "C").unwrap();
    assert_eq!(store.saved(a.id).unwrap().name, "C");
    assert_eq!(store.rename(a.id, "b").unwrap_err(), StoreError::NameTaken);
    assert_eq!(store.rename(a.id, " ").unwrap_err(), StoreError::EmptyName);
    assert_eq!(store.rename(Uuid::new_v4(), "Q").unwrap_err(), StoreError::NotFound);
}

#[test]
fn duplicate_naming() {
    let (_dir, store) = temp_store();
    let x = store.save_new("X", &example_profile()).unwrap();
    let c1 = store.duplicate(x.id, ProfileStore::default_copy_name).unwrap();
    let c2 = store.duplicate(x.id, ProfileStore::default_copy_name).unwrap();
    assert_eq!(c1.name, "X copy");
    assert_eq!(c2.name, "X copy 2");
    assert_ne!(c1.id, x.id);
    assert_eq!(c1.profile, example_profile());
    assert_eq!(store.list().len(), 3);
    assert_eq!(store.duplicate(Uuid::new_v4(), ProfileStore::default_copy_name).unwrap_err(), StoreError::NotFound);
}

#[test]
fn duplicate_uses_injected_copy_name() {
    let (_dir, store) = temp_store();
    let x = store.save_new("X", &flat()).unwrap();
    let name = |base: &str, n: usize| if n == 1 { format!("{base} Kopie") } else { format!("{base} Kopie {n}") };
    assert_eq!(store.duplicate(x.id, name).unwrap().name, "X Kopie");
    assert_eq!(store.duplicate(x.id, name).unwrap().name, "X Kopie 2");
    assert_eq!(store.duplicate(x.id, ProfileStore::default_copy_name).unwrap().name, "X copy");
}

#[test]
fn delete_removes() {
    let (_dir, store) = temp_store();
    let x = store.save_new("X", &flat()).unwrap();
    store.delete(x.id).unwrap();
    assert!(store.list().is_empty());
    assert_eq!(store.delete(x.id).unwrap_err(), StoreError::NotFound);
    assert_eq!(store.delete(Uuid::new_v4()).unwrap_err(), StoreError::NotFound);
}

#[test]
fn corrupt_and_wrong_version_files_are_skipped() {
    let (dir, store) = temp_store();
    let good = store.save_new("Bra", &flat()).unwrap();
    let corrupt = store.save_new("Trasig", &flat()).unwrap();
    let future = store.save_new("Framtid", &flat()).unwrap();
    std::fs::write(profile_file(dir.path(), corrupt.id), b"{").unwrap();
    let url = profile_file(dir.path(), future.id);
    let text = std::fs::read_to_string(&url).unwrap();
    assert!(text.contains("\"version\""));
    let mut obj: serde_json::Value = serde_json::from_str(&text).unwrap();
    obj["version"] = 2.into();
    std::fs::write(&url, obj.to_string()).unwrap();
    assert_eq!(store.list().iter().map(|s| s.id).collect::<Vec<_>>(), [good.id]);
    assert!(store.saved(corrupt.id).is_none());
    assert!(store.saved(future.id).is_none());
}

#[test]
fn working_copy_round_trip() {
    let (_dir, store) = temp_store();
    let id = Uuid::new_v4();
    store.save_working_copy(&WorkingCopy { profile: example_profile(), source_id: Some(id) });
    assert_eq!(store.load_working_copy(), Some(WorkingCopy { profile: example_profile(), source_id: Some(id) }));
    store.save_working_copy(&WorkingCopy { profile: flat(), source_id: None });
    assert_eq!(store.load_working_copy(), Some(WorkingCopy { profile: flat(), source_id: None }));
}

#[test]
fn working_copy_missing_or_corrupt_is_nil() {
    let (dir, store) = temp_store();
    assert_eq!(store.load_working_copy(), None);
    store.save_working_copy(&WorkingCopy { profile: flat(), source_id: None });
    std::fs::write(dir.path().join("current.json"), b"nope").unwrap();
    assert_eq!(store.load_working_copy(), None);
}

fn sample_record() -> HearingTestRecord {
    HearingTestRecord {
        date: UNIX_EPOCH + Duration::from_secs(1_700_000_000),
        output: OutputDevice { uid: "u1".into(), name: "AirPods Pro".into() },
        calibration: TestCalibration::nominal(Transducer::InEar),
        left: vec![TestPoint { frequency_hz: 1000.0, dbfs: -50.0, hl: 30.0 }],
        right: vec![TestPoint { frequency_hz: 1000.0, dbfs: -55.0, hl: 25.0 }],
        flags: vec![TestFlag::Asymmetry],
        app_version: "2.2.0".into(),
    }
}

#[test]
fn assignments_round_trip() {
    let (_dir, store) = temp_store();
    assert_eq!(store.load_assignments(), OutputAssignments::default());
    let mut a = OutputAssignments::default();
    a.assign(
        &OutputDevice { uid: "x".into(), name: "X".into() },
        Some(Uuid::new_v4()),
        Some(LevelSettings { strength: 1.0, input_gain_db: 0.0, output_gain_db: 0.0, tone: 0.0, mpo_dbfs: -3.0 }),
    );
    store.save_assignments(&a);
    assert_eq!(store.load_assignments(), a);
}

#[test]
fn active_outputs_store_round_trip() {
    let (dir, store) = temp_store();
    assert_eq!(store.load_active_outputs(), ActiveOutputs::default());
    let a = ActiveOutputs { devices: vec![OutputDevice { uid: "airpods-uid".into(), name: "AirPods Pro".into() }] };
    store.save_active_outputs(&a);
    assert_eq!(store.load_active_outputs(), a);
    std::fs::write(dir.path().join("outputs.json"), b"not json").unwrap();
    assert_eq!(store.load_active_outputs(), ActiveOutputs::default());
}

#[test]
fn measurement_round_trips() {
    let (_dir, store) = temp_store();
    let id = Uuid::new_v4();
    store.save_new_with(id, "Tested", &example_profile(), Some(sample_record()), false).unwrap();
    assert_eq!(store.saved(id).unwrap().measurement, Some(sample_record()));
    let plain = store.save_new("Plain", &example_profile()).unwrap();
    assert_eq!(store.saved(plain.id).unwrap().measurement, None);
}

#[test]
fn unique_name_appends_suffix() {
    let (_dir, store) = temp_store();
    let n = "AirPods Pro – 16 Nov 2026";
    let a = store.save_new_with(Uuid::new_v4(), n, &flat(), None, true).unwrap();
    let b = store.save_new_with(Uuid::new_v4(), &n.to_uppercase(), &flat(), None, true).unwrap();
    let c = store.save_new_with(Uuid::new_v4(), n, &flat(), None, true).unwrap();
    assert_eq!(a.name, n);
    assert_eq!(b.name, n.to_uppercase() + " (2)");
    assert_eq!(c.name, format!("{n} (3)"));
}

#[test]
fn unique_name_truncates_before_suffix() {
    let (_dir, store) = temp_store();
    let base = format!("{} {}", "a".repeat(55), "b".repeat(14));
    store.save_new_with(Uuid::new_v4(), &base, &flat(), None, true).unwrap();
    let second = store.save_new_with(Uuid::new_v4(), &base, &flat(), None, true).unwrap();
    assert!(second.name.chars().count() <= ProfileStore::MAX_NAME_LENGTH);
    assert!(second.name.ends_with(" (2)"));
    assert!(!second.name.contains("  "));
    let spaced = format!("{} dddd", "c".repeat(56));
    store.save_new_with(Uuid::new_v4(), &spaced, &flat(), None, true).unwrap();
    let s2 = store.save_new_with(Uuid::new_v4(), &spaced, &flat(), None, true).unwrap();
    assert_eq!(s2.name, format!("{} (2)", "c".repeat(56)));
}

#[test]
fn save_new_uses_given_id() {
    let (_dir, store) = temp_store();
    let id = Uuid::new_v4();
    let s = store.save_new_with(id, "X", &flat(), None, false).unwrap();
    assert_eq!(s.id, id);
    assert!(store.saved(id).is_some());
}

#[test]
fn non_ascii_names_truncate_on_char_boundaries() {
    let (_dir, store) = temp_store();
    let name = format!("{}Å😀", "a".repeat(59));
    let s = store.save_new(&name, &flat()).unwrap();
    assert_eq!(s.name, format!("{}Å", "a".repeat(59)));
}

#[test]
fn list_uses_natural_case_insensitive_order() {
    let (_dir, store) = temp_store();
    for n in ["Profile 10", "banana", "Profile 2", "apple"] {
        store.save_new(n, &flat()).unwrap();
    }
    let names: Vec<String> = store.list().into_iter().map(|s| s.name).collect();
    assert_eq!(names, ["apple", "banana", "Profile 2", "Profile 10"]);
}

#[test]
fn missing_root_and_leftover_temp_files() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("does/not/exist");
    let store = ProfileStore::new(root.clone());
    assert!(store.list().is_empty());
    let s = store.save_new("A", &flat()).unwrap();
    std::fs::write(root.join("profiles/.junk.json.tmp"), b"{").unwrap();
    assert_eq!(store.list().iter().map(|p| p.id).collect::<Vec<_>>(), [s.id]);
}

#[test]
fn out_of_range_timestamps_skip_the_file_instead_of_panicking() {
    let (dir, store) = temp_store();
    let good = store.save_new("Good", &flat()).unwrap();
    for (name, secs) in [("Huge", 1e30), ("Negative", -1e30)] {
        let s = store.save_new(name, &flat()).unwrap();
        let path = profile_file(dir.path(), s.id);
        let mut obj: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        obj["value"]["modified"] = serde_json::json!(secs);
        std::fs::write(&path, obj.to_string()).unwrap();
    }
    assert_eq!(store.list().iter().map(|s| s.id).collect::<Vec<_>>(), [good.id]);
}

#[test]
fn concurrent_writers_of_one_file_all_succeed() {
    let (_dir, store) = temp_store();
    let id = store.save_new("Shared", &flat()).unwrap().id;
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let store = store.clone();
            std::thread::spawn(move || (0..50).map(|_| store.overwrite(id, &example_profile())).collect::<Vec<_>>())
        })
        .collect();
    for h in handles {
        for r in h.join().unwrap() {
            assert_eq!(r, Ok(()));
        }
    }
    assert_eq!(store.saved(id).unwrap().profile, example_profile());
}
