//! An instance's "I am here" note, written to the shared store every `INTERVAL` so others can
//! tell when another host also processes the sound (Swift `InstanceHeartbeat`).

use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct InstanceHeartbeat {
    pub pid: u32,
    /// The host's bundle ID, or "unknown" until the host identifies itself.
    pub host_bundle_id: String,
    /// Processing is on for the current output.
    pub active: bool,
    #[serde(with = "crate::time_serde")]
    pub time: SystemTime,
}

impl InstanceHeartbeat {
    pub const INTERVAL: Duration = Duration::from_secs(2);
    pub const STALE_AFTER: Duration = Duration::from_secs(6);
    /// Older than this, a heartbeat file is deleted.
    pub const PRUNE_AFTER: Duration = Duration::from_secs(60);

    /// Time since the beat; zero for a beat from the future.
    pub fn age(&self, now: SystemTime) -> Duration {
        now.duration_since(self.time).unwrap_or(Duration::ZERO)
    }

    /// Active beats no older than `STALE_AFTER`, other than `excluding_pid`, sorted by host bundle ID.
    pub fn live(beats: &[InstanceHeartbeat], now: SystemTime, excluding_pid: u32) -> Vec<InstanceHeartbeat> {
        let mut out: Vec<_> = beats
            .iter()
            .filter(|b| b.pid != excluding_pid && b.active && b.age(now) <= Self::STALE_AFTER)
            .cloned()
            .collect();
        out.sort_by(|a, b| (&a.host_bundle_id, a.pid).cmp(&(&b.host_bundle_id, b.pid)));
        out
    }
}
