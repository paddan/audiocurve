//! One-time migration of the Swift app's `UserDefaults` into the Rust app's store: a hearing
//! test's unrestored volume snapshot and the settings. The parsing and merging here are
//! pure; the macOS side that reads `NSUserDefaults` is `platform::macos::migrate`.

use std::collections::BTreeMap;
use std::path::Path;

use audiocurve_core::store::ProfileStore;
use audiocurve_core::volume::VolumeSnapshot;
use serde_json::Value;

use crate::model::settings::Settings;

/// What the Swift app kept in `UserDefaults`; a key it never wrote is `None`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SwiftDefaults {
    pub enabled: Option<bool>,
    pub background_processing: Option<bool>,
    pub show_in_menu_bar: Option<bool>,
    /// `askedLaunchAtLogin`: the one-time launch-at-login question was asked.
    pub asked_launch_at_login: Option<bool>,
    /// The raw JSON of `hearingTest.pendingVolume`.
    pub pending_volume: Option<Vec<u8>>,
}

/// The snapshots in Swift's `hearingTest.pendingVolume`: a JSON array, or the single object
/// that versions before 2.2 wrote. Each is `{"deviceUID", "scalars": [k0, v0, k1, v1, …],
/// "failedAttempts"?}` (Swift encodes a `[UInt32: Float32]` as a flat key/value array).
/// Anything else gives nothing; a malformed snapshot in an array is skipped, so one bad entry
/// does not lose the others.
pub fn parse_pending_volumes(json: &[u8]) -> Vec<VolumeSnapshot> {
    match serde_json::from_slice::<Value>(json) {
        Ok(Value::Array(all)) => all.iter().filter_map(parse_snapshot).collect(),
        Ok(one @ Value::Object(_)) => parse_snapshot(&one).into_iter().collect(),
        _ => Vec::new(),
    }
}

fn parse_snapshot(v: &Value) -> Option<VolumeSnapshot> {
    let device_uid = v.get("deviceUID")?.as_str()?.to_owned();
    let flat = v.get("scalars")?.as_array()?;
    if flat.len() % 2 != 0 {
        return None;
    }
    let mut scalars = BTreeMap::new();
    for pair in flat.as_chunks::<2>().0 {
        let element = u32::try_from(pair[0].as_u64()?).ok()?;
        let volume = pair[1].as_f64()? as f32;
        scalars.insert(element, volume);
    }
    if scalars.is_empty() {
        return None;
    }
    let failed_attempts = v
        .get("failedAttempts")
        .and_then(Value::as_u64)
        .and_then(|n| u32::try_from(n).ok())
        .unwrap_or(0);
    Some(VolumeSnapshot {
        device_uid,
        scalars,
        failed_attempts,
    })
}

/// Merges the Swift defaults into the store. A pending snapshot is saved with core's
/// `save_pending_volume`, so an original already in the store is never replaced. The settings
/// (and whether the launch-at-login question was asked) are written only when `settings.json` does not exist yet. Returns whether the Swift pending
/// key should now be removed: it was present and every snapshot in it is confirmed in the store
/// (or it is too corrupt to hold any). False when absent or when a write did not stick.
pub fn migrate(d: &SwiftDefaults, store: &ProfileStore, settings_root: &Path) -> bool {
    if !settings_root.join("settings.json").exists() {
        let mut s = Settings::default();
        s.enabled = d.enabled.unwrap_or(s.enabled);
        s.background_processing = d.background_processing.unwrap_or(s.background_processing);
        s.show_in_menu_bar = d.show_in_menu_bar.unwrap_or(s.show_in_menu_bar);
        s.asked_launch_at_login = d.asked_launch_at_login.unwrap_or(s.asked_launch_at_login);
        if d.enabled.is_some()
            || d.background_processing.is_some()
            || d.show_in_menu_bar.is_some()
            || d.asked_launch_at_login.is_some()
        {
            s.save(settings_root);
        }
    }
    let Some(json) = &d.pending_volume else {
        return false;
    };
    let snapshots = parse_pending_volumes(json);
    for snapshot in &snapshots {
        store.save_pending_volume(snapshot.clone());
    }
    // `save_pending_volume` reports no error: read back, so a failed write keeps the Swift key
    // (the only copy of the user's original volume) for the next launch to retry.
    snapshots.iter().all(|s| {
        store
            .pending_volume(&s.device_uid)
            .is_some_and(|saved| s.scalars.keys().all(|k| saved.scalars.contains_key(k)))
    })
}
