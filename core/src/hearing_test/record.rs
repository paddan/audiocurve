//! What a hearing test measured, kept with the profile it produced (Swift `HearingTestRecord`).

use std::time::SystemTime;

use serde::{Deserialize, Serialize};

use super::{TestCalibration, TestFlag, TestPoint};
use crate::outputs::OutputDevice;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HearingTestRecord {
    #[serde(with = "crate::time_serde")]
    pub date: SystemTime,
    pub output: OutputDevice,
    pub calibration: TestCalibration,
    pub left: Vec<TestPoint>,
    pub right: Vec<TestPoint>,
    pub flags: Vec<TestFlag>,
    pub app_version: String,
}

impl HearingTestRecord {
    /// "<device> – <date>", the suggested name for a measured profile. The UI formats the date.
    pub fn default_name(device_name: &str, formatted_date: &str) -> String {
        format!("{device_name} – {formatted_date}")
    }
}
