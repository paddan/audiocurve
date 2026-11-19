//! An output device's hardware volume saved before the hearing test raises it to max, so it can
//! be restored afterwards, also after a crash (Swift `VolumeSnapshot`).
//!
//! Invariant: a pending snapshot holds the user's original volume and is never replaced while it
//! exists; it only shrinks as elements are restored or given up, and is cleared when none remain.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VolumeSnapshot {
    pub device_uid: String,
    /// Element (0 = main, 1, 2 = channels) → volume scalar 0…1.
    pub scalars: BTreeMap<u32, f32>,
    /// Restore attempts in which some element's write failed.
    #[serde(default)]
    pub failed_attempts: u32,
}

/// How writing one element back went.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ElementRestore {
    Restored,
    /// The element is gone or no longer accepts writes: nothing to retry.
    NotWritable,
    /// The write was refused; retried (a reconnected device may not be ready yet).
    Failed,
}

/// Attempts with a failed write before the remaining elements are given up, so a write that
/// keeps failing does not keep overriding the user's volume keys.
pub const MAX_RESTORE_ATTEMPTS: u32 = 5;

impl VolumeSnapshot {
    /// What is still to restore after one attempt with these per-element outcomes, or None when
    /// the restore is done: every element was restored, does not accept writes, or failed
    /// `MAX_RESTORE_ATTEMPTS` times. Elements missing from `outcomes` count as not writable.
    pub fn remaining(&self, outcomes: &BTreeMap<u32, ElementRestore>) -> Option<VolumeSnapshot> {
        let failed: BTreeMap<u32, f32> = self
            .scalars
            .iter()
            .filter(|(k, _)| outcomes.get(k) == Some(&ElementRestore::Failed))
            .map(|(&k, &v)| (k, v))
            .collect();
        if failed.is_empty() {
            return None;
        }
        let attempts = self.failed_attempts + 1;
        (attempts < MAX_RESTORE_ATTEMPTS).then(|| VolumeSnapshot {
            device_uid: self.device_uid.clone(),
            scalars: failed,
            failed_attempts: attempts,
        })
    }
}

/// Records the volume to restore for `s.device_uid` in `all` (one snapshot per device). If that
/// device already has a pending snapshot, it is the original and its values are never replaced:
/// only elements it lacks are added from `s`.
pub fn merge_pending(all: &mut Vec<VolumeSnapshot>, s: VolumeSnapshot) {
    match all.iter_mut().find(|p| p.device_uid == s.device_uid) {
        Some(p) => {
            for (k, v) in s.scalars {
                p.scalars.entry(k).or_insert(v);
            }
        }
        None => all.push(s),
    }
}
