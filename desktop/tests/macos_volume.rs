//! Touches the real output volume: runs only with `AUDIOCURVE_HW_TESTS=1`.
#![cfg(target_os = "macos")]

use audiocurve_app::platform::Volume;
use audiocurve_app::platform::macos::{hal, volume::MacVolume};
use audiocurve_core::volume::{ElementRestore, VolumeSnapshot};

fn enabled() -> bool {
    std::env::var("AUDIOCURVE_HW_TESTS").as_deref() == Ok("1")
}

fn default_uid() -> String {
    hal::device_uid(hal::default_output().expect("a default output")).expect("a UID")
}

/// Puts the original scalars back on drop, even when an assertion fails.
struct Guard(VolumeSnapshot);

impl Drop for Guard {
    fn drop(&mut self) {
        MacVolume.restore(&self.0);
    }
}

#[test]
fn snapshot_set_max_and_restore_round_trip() {
    if !enabled() {
        return;
    }
    let uid = default_uid();
    let Some(snap) = MacVolume.snapshot(&uid) else {
        eprintln!("default output has no settable volume; nothing to test");
        return;
    };
    let _guard = Guard(snap.clone());
    let mut vol = MacVolume;

    assert!(vol.set_max(&uid));
    let raised = vol.snapshot(&uid).expect("snapshot while raised");
    for (e, v) in &raised.scalars {
        assert!(*v >= 0.99, "element {e} at {v}");
    }

    let outcomes = vol.restore(&snap);
    assert!(outcomes.values().all(|o| *o == ElementRestore::Restored));
    let after = vol.snapshot(&uid).expect("snapshot after restore");
    assert_eq!(
        after.scalars.keys().collect::<Vec<_>>(),
        snap.scalars.keys().collect::<Vec<_>>()
    );
    for (e, v) in &snap.scalars {
        assert!(
            (after.scalars[e] - v).abs() < 0.001,
            "element {e}: {} vs {v}",
            after.scalars[e]
        );
    }
}

#[test]
fn is_muted_and_is_settable_answer() {
    if !enabled() {
        return;
    }
    let uid = default_uid();
    let vol = MacVolume;
    let _ = vol.is_muted(&uid);
    assert_eq!(vol.is_settable(&uid), vol.snapshot(&uid).is_some());
    assert!(!vol.is_muted("no-such-device-uid"));
    assert!(!vol.is_settable("no-such-device-uid"));
}
