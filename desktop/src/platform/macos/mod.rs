//! The real platform on macOS: Core Audio HAL, process taps and the app lifecycle.

pub mod app_menu;
pub mod audio;
pub mod capture_check;
pub mod diag;
pub mod hal;
pub mod migrate;
pub mod power;
pub mod route;
pub mod system;
pub mod tone;
pub mod tray;
pub mod volume;

use std::sync::Arc;

use super::{Platform, Tray};

/// Assembles the macOS platform. `tray` is passed in because a `MacTray` can only be made
/// on the main thread once the event loop runs. Builds no route and touches no volume.
pub fn mac_platform(waker: Arc<dyn Fn() + Send + Sync>, tray: Box<dyn Tray>) -> Platform {
    Platform {
        audio: Box::new(audio::MacAudio::new(waker)),
        volume: Box::new(volume::MacVolume),
        tone: Box::new(tone::MacTestTone::new()),
        activity: Box::new(system::MacActivity),
        system: Box::new(system::MacSystem),
        tray,
    }
}
