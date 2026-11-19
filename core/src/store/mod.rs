//! Named profile versions, an autosaved working copy, outputs, assignments, instance heartbeats
//! and pending volume snapshots, as JSON files under a root directory (Swift `ProfileStore`).
//! Keeps no cache, because other instances may write the same directory.

mod files;
mod names;
pub mod types;

use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use uuid::Uuid;

use crate::heartbeat::InstanceHeartbeat;
use crate::hearing_test::HearingTestRecord;
use crate::outputs::{ActiveOutputs, OutputAssignments};
use crate::profile::HearingProfile;
use crate::volume::{VolumeSnapshot, merge_pending};
use types::{SavedProfile, WorkingCopy};

pub use names::natural_cmp;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StoreError {
    EmptyName,
    NameTaken,
    NotFound,
    Io(String),
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StoreError::EmptyName => write!(f, "empty name"),
            StoreError::NameTaken => write!(f, "name taken"),
            StoreError::NotFound => write!(f, "not found"),
            StoreError::Io(m) => write!(f, "{m}"),
        }
    }
}

impl std::error::Error for StoreError {}

#[derive(Clone, Debug)]
pub struct ProfileStore {
    root: PathBuf,
}

impl ProfileStore {
    pub const FILE_VERSION: u32 = files::FILE_VERSION;
    pub const MAX_NAME_LENGTH: usize = names::MAX_NAME_LENGTH;

    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    /// The store under the platform's per-user data directory.
    pub fn standard() -> Self {
        Self::new(Self::default_root())
    }

    /// macOS `~/Library/Application Support/audiocurve`, Linux `$XDG_DATA_HOME/audiocurve`
    /// (or `~/.local/share/audiocurve`), Windows `%APPDATA%\audiocurve`.
    pub fn default_root() -> PathBuf {
        let env = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
        let home = env("HOME");
        let base = if cfg!(target_os = "macos") {
            home.map(|h| h.join("Library/Application Support"))
        } else if cfg!(target_os = "windows") {
            env("APPDATA")
        } else {
            env("XDG_DATA_HOME").or_else(|| home.map(|h| h.join(".local/share")))
        };
        base.unwrap_or_else(std::env::temp_dir).join("audiocurve")
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn profiles_dir(&self) -> PathBuf {
        self.root.join("profiles")
    }
    fn profile_path(&self, id: Uuid) -> PathBuf {
        self.profiles_dir().join(format!("{id}.json"))
    }
    fn working_copy_path(&self) -> PathBuf {
        self.root.join("current.json")
    }
    fn active_outputs_path(&self) -> PathBuf {
        self.root.join("outputs.json")
    }
    fn assignments_path(&self) -> PathBuf {
        self.root.join("assignments.json")
    }
    fn instances_dir(&self) -> PathBuf {
        self.root.join("instances")
    }
    fn heartbeat_path(&self, pid: u32) -> PathBuf {
        self.instances_dir().join(format!("{pid}.json"))
    }
    fn pending_volume_path(&self) -> PathBuf {
        self.root.join("pending-volume.json")
    }

    /// The `*.json` files in `dir` (temporary files start with "." and end in ".tmp").
    fn json_files(dir: &Path) -> Vec<PathBuf> {
        let Ok(entries) = fs::read_dir(dir) else { return vec![] };
        entries
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|x| x == "json"))
            .filter(|p| !p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with('.')))
            .collect()
    }

    // Versions

    pub fn list(&self) -> Vec<SavedProfile> {
        let mut all: Vec<SavedProfile> = Self::json_files(&self.profiles_dir())
            .iter()
            .filter_map(|p| files::read::<SavedProfile>(p))
            .filter(|s| s.profile.validate().is_ok())
            .collect();
        all.sort_by(|a, b| natural_cmp(&a.name, &b.name));
        all
    }

    pub fn saved(&self, id: Uuid) -> Option<SavedProfile> {
        files::read::<SavedProfile>(&self.profile_path(id)).filter(|s| s.profile.validate().is_ok())
    }

    pub fn save_new(&self, name: &str, profile: &HearingProfile) -> Result<SavedProfile, StoreError> {
        self.save_new_with(Uuid::new_v4(), name, profile, None, false)
    }

    /// With `unique_name`, a taken name gets " (2)", " (3)", … instead of `NameTaken`.
    pub fn save_new_with(
        &self,
        id: Uuid,
        name: &str,
        profile: &HearingProfile,
        measurement: Option<HearingTestRecord>,
        unique_name: bool,
    ) -> Result<SavedProfile, StoreError> {
        check_valid(profile)?;
        let all = self.list();
        let taken = all.iter().map(|s| s.name.as_str());
        let name = if unique_name { names::unique_clean_name(name, taken)? } else { names::clean_name(name, taken)? };
        let s = SavedProfile { id, name, modified: SystemTime::now(), profile: profile.clone(), measurement };
        files::write(&self.profile_path(id), &s)?;
        Ok(s)
    }

    pub fn overwrite(&self, id: Uuid, profile: &HearingProfile) -> Result<(), StoreError> {
        check_valid(profile)?;
        let mut s = self.saved(id).ok_or(StoreError::NotFound)?;
        s.profile = profile.clone();
        s.modified = SystemTime::now();
        files::write(&self.profile_path(id), &s)
    }

    pub fn rename(&self, id: Uuid, name: &str) -> Result<(), StoreError> {
        let mut s = self.saved(id).ok_or(StoreError::NotFound)?;
        let all = self.list();
        s.name = names::clean_name(name, all.iter().filter(|o| o.id != id).map(|o| o.name.as_str()))?;
        s.modified = SystemTime::now();
        files::write(&self.profile_path(id), &s)
    }

    /// Name for the `n`th copy of `base` (n = 1 for the first). The UI passes a localized one;
    /// this default is the English source.
    pub fn default_copy_name(base: &str, n: usize) -> String {
        if n == 1 { format!("{base} copy") } else { format!("{base} copy {n}") }
    }

    pub fn duplicate(&self, id: Uuid, copy_name: impl Fn(&str, usize) -> String) -> Result<SavedProfile, StoreError> {
        let s = self.saved(id).ok_or(StoreError::NotFound)?;
        let taken: Vec<String> = self.list().into_iter().map(|o| o.name.to_lowercase()).collect();
        let candidate = (1..)
            .map(|n| copy_name(&s.name, n))
            .find(|c| !taken.contains(&c.to_lowercase()))
            .ok_or(StoreError::NameTaken)?;
        self.save_new(&candidate, &s.profile)
    }

    pub fn delete(&self, id: Uuid) -> Result<(), StoreError> {
        let path = self.profile_path(id);
        if !path.exists() {
            return Err(StoreError::NotFound);
        }
        fs::remove_file(path).map_err(|e| StoreError::Io(e.to_string()))
    }

    // Working copy

    pub fn load_working_copy(&self) -> Option<WorkingCopy> {
        files::read::<WorkingCopy>(&self.working_copy_path()).filter(|w| w.profile.validate().is_ok())
    }

    /// Best effort, never fails.
    pub fn save_working_copy(&self, w: &WorkingCopy) {
        let _ = files::write(&self.working_copy_path(), w);
    }

    // Active outputs and assignments

    /// Missing or corrupt file → all outputs (the default).
    pub fn load_active_outputs(&self) -> ActiveOutputs {
        files::read(&self.active_outputs_path()).unwrap_or_default()
    }

    /// Best effort, never fails.
    pub fn save_active_outputs(&self, a: &ActiveOutputs) {
        let _ = files::write(&self.active_outputs_path(), a);
    }

    /// Missing or corrupt file → no assignments.
    pub fn load_assignments(&self) -> OutputAssignments {
        files::read(&self.assignments_path()).unwrap_or_default()
    }

    /// Best effort, never fails.
    pub fn save_assignments(&self, a: &OutputAssignments) {
        let _ = files::write(&self.assignments_path(), a);
    }

    // Instance heartbeats

    /// Writes `instances/<pid>.json`. Best effort, never fails.
    pub fn write_heartbeat(&self, b: &InstanceHeartbeat) {
        let _ = files::write(&self.heartbeat_path(b.pid), b);
    }

    /// Every readable heartbeat, sorted by pid; unreadable files are skipped.
    pub fn read_heartbeats(&self) -> Vec<InstanceHeartbeat> {
        let mut all: Vec<InstanceHeartbeat> =
            Self::json_files(&self.instances_dir()).iter().filter_map(|p| files::read(p)).collect();
        all.sort_by_key(|b| b.pid);
        all
    }

    /// Removes heartbeats older than `InstanceHeartbeat::PRUNE_AFTER` (instances that died
    /// without cleaning up). Best effort.
    pub fn prune_heartbeats(&self, now: SystemTime) {
        for b in self.read_heartbeats() {
            if b.age(now) > InstanceHeartbeat::PRUNE_AFTER {
                self.remove_heartbeat(b.pid);
            }
        }
    }

    pub fn remove_heartbeat(&self, pid: u32) {
        let _ = fs::remove_file(self.heartbeat_path(pid));
    }

    // Pending volume snapshots (hearing test)

    /// The volumes still waiting to be restored, at most one per device. Corrupt file → none.
    pub fn pending_volumes(&self) -> Vec<VolumeSnapshot> {
        files::read(&self.pending_volume_path()).unwrap_or_default()
    }

    pub fn pending_volume(&self, uid: &str) -> Option<VolumeSnapshot> {
        self.pending_volumes().into_iter().find(|s| s.device_uid == uid)
    }

    /// Records the volume to restore, written before the volume is raised. An existing snapshot
    /// for the device is the original and is never replaced: only missing elements are added.
    pub fn save_pending_volume(&self, s: VolumeSnapshot) {
        let mut all = self.pending_volumes();
        merge_pending(&mut all, s);
        self.write_pending_volumes(&all);
    }

    /// After a restore attempt: keeps what is left of that device's snapshot, or forgets it.
    pub fn update_pending_volume(&self, uid: &str, remaining: Option<VolumeSnapshot>) {
        let mut all = self.pending_volumes();
        let Some(i) = all.iter().position(|s| s.device_uid == uid) else { return };
        match remaining {
            Some(r) => all[i] = r,
            None => {
                all.remove(i);
            }
        }
        self.write_pending_volumes(&all);
    }

    /// Forgets the snapshot of one device; the others stay.
    pub fn clear_pending_volume(&self, uid: &str) {
        let mut all = self.pending_volumes();
        all.retain(|s| s.device_uid != uid);
        self.write_pending_volumes(&all);
    }

    fn write_pending_volumes(&self, all: &[VolumeSnapshot]) {
        if all.is_empty() {
            let _ = fs::remove_file(self.pending_volume_path());
        } else {
            let _ = files::write(&self.pending_volume_path(), &all);
        }
    }
}

fn check_valid(p: &HearingProfile) -> Result<(), StoreError> {
    p.validate().map_err(|_| StoreError::Io("invalid profile".into()))
}
