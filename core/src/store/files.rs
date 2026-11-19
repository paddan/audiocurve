//! Versioned JSON files with atomic writes.

use std::fs;
use std::path::Path;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use super::StoreError;

pub const FILE_VERSION: u32 = 1;

#[derive(Serialize)]
struct EnvelopeRef<'a, T> {
    version: u32,
    value: &'a T,
}

#[derive(Deserialize)]
struct Envelope<T> {
    version: u32,
    value: T,
}

/// The value in `path`, or None when the file is missing, unreadable, of another version or
/// does not parse.
pub fn read<T: DeserializeOwned>(path: &Path) -> Option<T> {
    let data = fs::read(path).ok()?;
    let env: Envelope<T> = serde_json::from_slice(&data).ok()?;
    (env.version == FILE_VERSION).then_some(env.value)
}

/// Writes `value` to `path` atomically: a temporary file in the same directory, then a rename.
pub fn write<T: Serialize>(path: &Path, value: &T) -> Result<(), StoreError> {
    let io = |e: std::io::Error| StoreError::Io(e.to_string());
    let dir = path.parent().ok_or_else(|| StoreError::Io("no parent directory".into()))?;
    fs::create_dir_all(dir).map_err(io)?;
    let data = serde_json::to_vec(&EnvelopeRef { version: FILE_VERSION, value })
        .map_err(|e| StoreError::Io(e.to_string()))?;
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
    // Unique per write, so concurrent writers (other processes or threads) never share one.
    let tmp = dir.join(format!(".{name}.{}.{}.tmp", std::process::id(), uuid::Uuid::new_v4().simple()));
    fs::write(&tmp, data).map_err(io)?;
    fs::rename(&tmp, path).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        io(e)
    })
}
