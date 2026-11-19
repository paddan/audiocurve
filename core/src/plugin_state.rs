//! The profile a plugin instance keeps in its host's saved state (Swift `PluginState`).

use serde::{Deserialize, Serialize};

use crate::profile::HearingProfile;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PluginState {
    pub version: u32,
    pub profile: Option<HearingProfile>,
}

impl PluginState {
    pub const CURRENT_VERSION: u32 = 1;

    pub fn encode(&self) -> Vec<u8> {
        serde_json::to_vec(self).unwrap_or_default()
    }

    /// Never fails. Missing, corrupt, unknown-version or invalid data yields `profile == None`.
    pub fn decode(data: Option<&[u8]>) -> PluginState {
        let empty = PluginState { version: Self::CURRENT_VERSION, profile: None };
        let Some(state) = data.and_then(|d| serde_json::from_slice::<PluginState>(d).ok()) else { return empty };
        match &state.profile {
            Some(p) if state.version == Self::CURRENT_VERSION && p.validate().is_ok() => state,
            _ => empty,
        }
    }

    /// Which profile an instance should hold after a host restores its state: a host-supplied
    /// profile wins, but an empty or foreign state must not wipe the one it already has.
    pub fn resolve(host_profile: Option<HearingProfile>, current: Option<HearingProfile>) -> Option<HearingProfile> {
        host_profile.or(current)
    }
}
