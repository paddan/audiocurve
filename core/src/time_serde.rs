//! `SystemTime` stored as Unix seconds (`f64`): `#[serde(with = "crate::time_serde")]`.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Deserializer, Serializer};

pub fn to_seconds(t: SystemTime) -> f64 {
    match t.duration_since(UNIX_EPOCH) {
        Ok(d) => d.as_secs_f64(),
        Err(e) => -e.duration().as_secs_f64(),
    }
}

/// The time `s` seconds from the Unix epoch, or None when it is not finite or out of range.
pub fn from_seconds(s: f64) -> Option<SystemTime> {
    let d = Duration::try_from_secs_f64(s.abs()).ok()?;
    if s >= 0.0 { UNIX_EPOCH.checked_add(d) } else { UNIX_EPOCH.checked_sub(d) }
}

pub fn serialize<S: Serializer>(t: &SystemTime, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_f64(to_seconds(*t))
}

pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<SystemTime, D::Error> {
    let secs = f64::deserialize(d)?;
    from_seconds(secs).ok_or_else(|| serde::de::Error::custom(format!("time out of range: {secs}")))
}
