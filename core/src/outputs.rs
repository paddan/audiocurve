//! Output devices, which outputs are active, and per-output assignments.

use std::collections::{BTreeMap, HashSet};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::profile::HearingProfile;
use crate::store::types::WorkingCopy;

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct OutputDevice {
    pub uid: String,
    pub name: String,
}

/// Which outputs AudioCurve processes. Empty means all.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ActiveOutputs {
    pub devices: Vec<OutputDevice>,
}

impl ActiveOutputs {
    pub fn is_all(&self) -> bool {
        self.devices.is_empty()
    }

    pub fn contains(&self, uid: &str) -> bool {
        self.devices.iter().any(|d| d.uid == uid)
    }

    /// Should processing run while `current_uid` is the system output? Unknown output (None) or
    /// no selection keeps processing on, so a lookup failure never silently turns the hearing
    /// compensation off.
    pub fn is_active(&self, current_uid: Option<&str>) -> bool {
        match current_uid {
            Some(uid) if !self.devices.is_empty() => self.contains(uid),
            _ => true,
        }
    }

    /// Is `uid` ticked in the list? With no selection every device is.
    pub fn is_checked(&self, uid: &str) -> bool {
        self.is_all() || self.contains(uid)
    }

    /// Is `uid` the only ticked device? It cannot be unticked, since an empty selection means all.
    pub fn is_last_checked(&self, uid: &str, available: &[OutputDevice]) -> bool {
        let mut checked = self.menu_devices(available).into_iter().filter(|d| self.is_checked(&d.uid));
        matches!((checked.next(), checked.next()), (Some(d), None) if d.uid == uid)
    }

    /// Ticks or unticks `device` in the list of `available` devices (plus remembered ones).
    /// Unticking from "all" keeps every other device; ticking every device returns to "all";
    /// the last ticked device stays ticked.
    pub fn set_enabled(&mut self, device: &OutputDevice, enabled: bool, available: &[OutputDevice]) {
        let shown = self.menu_devices(available);
        if enabled {
            if self.is_checked(&device.uid) {
                return;
            }
            self.devices.push(device.clone());
            if shown.iter().all(|d| self.contains(&d.uid)) {
                self.devices.clear();
            }
        } else {
            if !self.is_checked(&device.uid) || self.is_last_checked(&device.uid, available) {
                return;
            }
            if self.is_all() {
                self.devices = shown;
            }
            self.devices.retain(|d| d.uid != device.uid);
        }
    }

    /// Devices to list: the ones currently available, then remembered ones that are not.
    pub fn menu_devices(&self, available: &[OutputDevice]) -> Vec<OutputDevice> {
        let mut out = available.to_vec();
        out.extend(self.devices.iter().filter(|d| !available.iter().any(|a| a.uid == d.uid)).cloned());
        out
    }
}

/// The level controls remembered per output.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct LevelSettings {
    pub strength: f64,
    pub input_gain_db: f64,
    pub output_gain_db: f64,
    pub tone: f64,
    pub mpo_dbfs: f64,
}

/// What an output is set up with: a saved profile and/or level settings.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OutputAssignment {
    pub device: OutputDevice,
    pub profile_id: Option<Uuid>,
    pub levels: Option<LevelSettings>,
}

/// Per-output assignments, keyed by `OutputDevice::uid`.
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct OutputAssignments {
    pub by_uid: BTreeMap<String, OutputAssignment>,
}

impl OutputAssignments {
    pub fn assignment(&self, uid: &str) -> Option<&OutputAssignment> {
        self.by_uid.get(uid)
    }

    pub fn assign(&mut self, device: &OutputDevice, profile: Option<Uuid>, levels: Option<LevelSettings>) {
        self.by_uid.insert(device.uid.clone(), OutputAssignment { device: device.clone(), profile_id: profile, levels });
    }

    /// Updates the levels of an already known output; unknown outputs are left alone.
    pub fn set_levels(&mut self, levels: LevelSettings, uid: &str) {
        if let Some(a) = self.by_uid.get_mut(uid) {
            a.levels = Some(levels);
        }
    }

    /// Clears references to deleted profiles and drops entries left with nothing to apply.
    pub fn prune(&mut self, existing: &HashSet<Uuid>) {
        self.by_uid.retain(|_, a| {
            if a.profile_id.is_some_and(|p| !existing.contains(&p)) {
                a.profile_id = None;
            }
            a.profile_id.is_some() || a.levels.is_some()
        });
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum OutputSwitchAction {
    Overwrite(Uuid, HearingProfile),
    SaveUnsaved(HearingProfile),
    Load(Uuid),
    ApplyLevels(LevelSettings),
}

/// What to do when the system output changes to one with `assignment`. `saved_source` is the
/// saved profile of `working_copy.source_id`: None if there is none or it was deleted.
pub fn output_switch_actions(
    assignment: Option<&OutputAssignment>,
    working_copy: Option<&WorkingCopy>,
    saved_source: Option<&HearingProfile>,
    existing: &HashSet<Uuid>,
) -> Vec<OutputSwitchAction> {
    let Some(assignment) = assignment else { return vec![] };
    let mut result = Vec::new();
    let levels = assignment.levels.map(OutputSwitchAction::ApplyLevels);
    match assignment.profile_id {
        Some(target) if existing.contains(&target) && Some(target) != working_copy.and_then(|w| w.source_id) => {
            if let Some(wc) = working_copy {
                match (wc.source_id, saved_source) {
                    (Some(source), Some(saved)) => {
                        if *saved != wc.profile {
                            result.push(OutputSwitchAction::Overwrite(source, wc.profile.clone()));
                        }
                    }
                    _ if wc.profile != HearingProfile::flat() => {
                        result.push(OutputSwitchAction::SaveUnsaved(wc.profile.clone()));
                    }
                    _ => {}
                }
            }
            // The new output's levels (and ceiling) before its profile, so a stronger profile
            // never runs with the old output's levels.
            result.extend(levels);
            result.push(OutputSwitchAction::Load(target));
        }
        _ => result.extend(levels),
    }
    result
}
