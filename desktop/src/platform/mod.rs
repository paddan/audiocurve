//! Everything platform-specific, behind traits. The model and UI only see these.

pub mod fake;
#[cfg(target_os = "macos")]
pub mod macos;

use std::collections::BTreeMap;

use audiocurve_core::outputs::OutputDevice;
use audiocurve_core::profile::Ear;
use audiocurve_core::route::RouteBuilder;
use audiocurve_core::volume::{ElementRestore, VolumeSnapshot};
use uuid::Uuid;

use crate::engine::SharedEngine;
use crate::model::menu::{MenuAction, MenuItem};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlatformEvent {
    OutputsChanged,
    SampleRateChanged,
    /// The route failed to start or its IO panicked (or kept stalling): counts toward the
    /// controller's death latch.
    RenderFailed {
        route_id: Uuid,
    },
    /// The running route's IO stopped calling back mid-run (as it can around sleep and wake):
    /// rebuild it, without counting toward the death latch.
    RouteStalled {
        route_id: Uuid,
    },
    CaptureLooksDenied,
    /// The running route's input carried sound: capture is permitted.
    CaptureWorks,
}

pub trait Audio {
    fn outputs(&self) -> Vec<OutputDevice>;
    fn current_output(&self) -> Option<OutputDevice>;
    fn route_builder(&self, engine: SharedEngine) -> Box<dyn RouteBuilder>;
    fn poll_events(&mut self) -> Vec<PlatformEvent>;
}

pub trait Volume {
    fn snapshot(&self, uid: &str) -> Option<VolumeSnapshot>;
    fn set_max(&mut self, uid: &str) -> bool;
    fn restore(&mut self, s: &VolumeSnapshot) -> BTreeMap<u32, ElementRestore>;
    fn is_muted(&self, uid: &str) -> bool;
    fn is_settable(&self, uid: &str) -> bool;
}

/// The real one renders core's `ToneSynth`.
pub trait TestTone {
    fn start(&mut self, uid: &str) -> Result<(), String>;
    fn play(&mut self, ear: Ear, frequency_hz: f64, level_dbfs: Option<f64>);
    /// The generator is started and running (Swift `ToneGenerator.isRunning`).
    fn is_playing(&self) -> bool;
    fn stop(&mut self);
    fn sample_rate(&self, uid: &str) -> f64;
}

pub trait Activity {
    fn others_playing(&self) -> bool;
}

pub trait System {
    fn launch_at_login(&self) -> bool;
    fn set_launch_at_login(&mut self, on: bool) -> Result<(), String>;
    fn set_dock_icon(&mut self, shown: bool);
    fn open_capture_settings(&mut self);
    fn app_name(&self, bundle_id: &str) -> String;
}

/// The menu bar ear (Swift `MenuBarIcon`): filled while processing, outline when paused or
/// starting, crossed out when off or failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrayIconState {
    Active,
    Inactive,
    Problem,
}

/// The menu bar item and its menu.
pub trait Tray {
    fn set_menu(&mut self, items: &[MenuItem]);
    fn set_visible(&mut self, visible: bool);
    fn set_icon(&mut self, icon: TrayIconState);
    /// The menu items clicked since the last call.
    fn poll_actions(&mut self) -> Vec<MenuAction>;
}

pub struct Platform {
    pub audio: Box<dyn Audio>,
    pub volume: Box<dyn Volume>,
    pub tone: Box<dyn TestTone>,
    pub activity: Box<dyn Activity>,
    pub system: Box<dyn System>,
    pub tray: Box<dyn Tray>,
}
