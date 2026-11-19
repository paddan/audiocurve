//! Types the store keeps.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::profile::HearingProfile;

/// The profile being edited, autosaved.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WorkingCopy {
    pub profile: HearingProfile,
    /// The saved version it was loaded from / last saved to.
    pub source_id: Option<Uuid>,
}

/// A named, saved profile version.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SavedProfile {
    pub id: Uuid,
    pub name: String,
    #[serde(with = "crate::time_serde")]
    pub modified: std::time::SystemTime,
    pub profile: HearingProfile,
    /// The hearing test this profile came from, if any.
    #[serde(default)]
    pub measurement: Option<crate::hearing_test::HearingTestRecord>,
}
