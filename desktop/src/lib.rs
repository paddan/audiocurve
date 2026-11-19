//! AudioCurve egui app: engine wrapper and platform abstraction.

pub mod engine;
pub mod i18n;
pub mod lifecycle;
pub mod migrate_swift;
pub mod model;
pub mod platform;
#[cfg(feature = "screenshots")]
pub mod screenshots;
pub mod ui;
