//! A small route log in `~/Library/Logs/AudioCurve/route.log`: route builds, starts, stalls and
//! failures, so a "The sound processing stopped" can be traced afterwards. Off until `enable`
//! (the app calls it; tests do not, so they never write to the user's log). Never called from
//! the IO block.

use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

use chrono::{Datelike as _, Timelike as _};

static ENABLED: AtomicBool = AtomicBool::new(false);

/// The log is started over when it grows past this.
const MAX_BYTES: u64 = 1 << 20;

pub fn enable() {
    ENABLED.store(true, Ordering::Relaxed);
}

fn path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join("Library/Logs/AudioCurve/route.log"))
}

/// Appends one timestamped line; failures are ignored.
pub fn log(msg: &str) {
    if !ENABLED.load(Ordering::Relaxed) {
        return;
    }
    let Some(path) = path() else { return };
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let too_big = fs::metadata(&path).is_ok_and(|m| m.len() > MAX_BYTES);
    let file = OpenOptions::new()
        .create(true)
        .append(!too_big)
        .write(true)
        .truncate(too_big)
        .open(&path);
    if let Ok(mut f) = file {
        let t = chrono::Local::now();
        let _ = writeln!(
            f,
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:03} {msg}",
            t.year(),
            t.month(),
            t.day(),
            t.hour(),
            t.minute(),
            t.second(),
            t.timestamp_subsec_millis(),
        );
    }
}
