use std::collections::BTreeMap;

use audiocurve_core::volume::{ElementRestore as R, MAX_RESTORE_ATTEMPTS, VolumeSnapshot, merge_pending};

fn snap(uid: &str, scalars: &[(u32, f32)], failed_attempts: u32) -> VolumeSnapshot {
    VolumeSnapshot { device_uid: uid.into(), scalars: scalars.iter().copied().collect(), failed_attempts }
}

fn outcomes(o: &[(u32, R)]) -> BTreeMap<u32, R> {
    o.iter().copied().collect()
}

#[test]
fn volume_snapshot_round_trips_through_json() {
    let s = snap("dev-1", &[(0, 0.5), (1, 0.25), (2, 0.75)], 0);
    let back: VolumeSnapshot = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
    assert_eq!(back, s);
}

#[test]
fn saving_the_same_device_again_keeps_the_original() {
    let mut all = Vec::new();
    let original = snap("a", &[(0, 0.2)], 0);
    merge_pending(&mut all, original.clone());
    merge_pending(&mut all, snap("a", &[(0, 0.5)], 0));
    assert_eq!(all, [original]);
}

#[test]
fn pending_original_is_never_replaced_by_max() {
    let mut all = Vec::new();
    let original = snap("a", &[(0, 0.3), (1, 0.3)], 0);
    merge_pending(&mut all, original.clone());
    merge_pending(&mut all, snap("a", &[(0, 1.0), (1, 1.0)], 0));
    assert_eq!(all.iter().find(|s| s.device_uid == "a"), Some(&original));
    all.retain(|s| s.device_uid != "a");
    let next = snap("a", &[(0, 0.6)], 0);
    merge_pending(&mut all, next.clone());
    assert_eq!(all, [next]);
}

#[test]
fn saving_again_adds_only_missing_elements() {
    let mut all = Vec::new();
    merge_pending(&mut all, snap("a", &[(1, 0.3)], 2));
    merge_pending(&mut all, snap("a", &[(0, 0.4), (1, 1.0), (2, 1.0)], 0));
    assert_eq!(all, [snap("a", &[(0, 0.4), (1, 0.3), (2, 1.0)], 2)]);
}

#[test]
fn restore_is_done_when_every_writable_element_was_restored() {
    let s = snap("a", &[(0, 0.2), (1, 0.3), (2, 0.4)], 0);
    assert_eq!(s.remaining(&outcomes(&[(0, R::Restored), (1, R::Restored), (2, R::Restored)])), None);
    assert_eq!(s.remaining(&outcomes(&[(0, R::Restored), (1, R::NotWritable), (2, R::NotWritable)])), None);
    assert_eq!(s.remaining(&BTreeMap::new()), None);
}

#[test]
fn failed_writes_are_kept_for_a_limited_number_of_attempts() {
    let mut left = Some(snap("a", &[(0, 0.2), (1, 0.3)], 0));
    let mut attempts = 0;
    while let Some(r) = left {
        attempts += 1;
        left = r.remaining(&outcomes(&[(0, R::Restored), (1, R::Failed)]));
        if let Some(l) = &left {
            assert_eq!(l.scalars, [(1, 0.3)].into_iter().collect());
            assert_eq!(l.device_uid, "a");
        }
        assert!(attempts <= 100);
    }
    assert_eq!(attempts, MAX_RESTORE_ATTEMPTS);
}

#[path = "support/mod.rs"]
mod support;

#[test]
fn pending_volume_is_saved_read_and_cleared() {
    let (_dir, store) = support::temp_store();
    assert!(store.pending_volumes().is_empty());
    let s = snap("dev-2", &[(1, 0.3), (2, 0.4)], 0);
    store.save_pending_volume(s.clone());
    assert_eq!(store.pending_volumes(), [s]);
    store.clear_pending_volume("dev-2");
    assert!(store.pending_volumes().is_empty());
}

#[test]
fn pending_volumes_of_two_devices_do_not_overwrite_each_other() {
    let (_dir, store) = support::temp_store();
    let a = snap("a", &[(0, 0.2)], 0);
    let b = snap("b", &[(0, 0.6)], 0);
    store.save_pending_volume(a.clone());
    store.save_pending_volume(b);
    let mut uids: Vec<String> = store.pending_volumes().into_iter().map(|s| s.device_uid).collect();
    uids.sort();
    assert_eq!(uids, ["a", "b"]);
    assert_eq!(store.pending_volume("a"), Some(a));
}

#[test]
fn failed_attempts_survive_saving_and_reading() {
    let (_dir, store) = support::temp_store();
    let after = snap("a", &[(1, 0.3)], 0).remaining(&outcomes(&[(1, R::Failed)])).unwrap();
    store.save_pending_volume(after.clone());
    assert_eq!(store.pending_volume("a"), Some(after.clone()));
    assert_eq!(after.failed_attempts, 1);
}

#[test]
fn update_pending_replaces_with_what_remains_or_clears() {
    let (_dir, store) = support::temp_store();
    let s = snap("a", &[(0, 0.2), (1, 0.3)], 0);
    let b = snap("b", &[(0, 0.6)], 0);
    store.save_pending_volume(s.clone());
    store.save_pending_volume(b.clone());
    let rest = s.remaining(&outcomes(&[(0, R::Restored), (1, R::Failed)]));
    store.update_pending_volume("a", rest.clone());
    assert_eq!(store.pending_volume("a"), rest);
    store.update_pending_volume("a", None);
    assert_eq!(store.pending_volumes(), [b]);
}

#[test]
fn clearing_one_device_keeps_the_other() {
    let (_dir, store) = support::temp_store();
    store.save_pending_volume(snap("a", &[(0, 0.2)], 0));
    let b = snap("b", &[(0, 0.6)], 0);
    store.save_pending_volume(b.clone());
    store.clear_pending_volume("a");
    assert_eq!(store.pending_volumes(), [b]);
}

#[test]
fn corrupt_pending_volume_reads_as_empty() {
    let (dir, store) = support::temp_store();
    std::fs::write(dir.path().join("pending-volume.json"), b"nope").unwrap();
    assert!(store.pending_volumes().is_empty());
}
