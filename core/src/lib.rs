//! Platform-independent AudioCurve logic, ported from the Swift `AudioCurveCore` package.

pub use audiocurve_dsp::{BAND_COUNT, CENTERS};

pub mod profile;
pub mod fitting;
pub mod editing;
pub mod hearing_test;
pub mod tone;
pub mod outputs;
pub mod time_serde;
pub mod store;
pub mod auto_level;
pub mod heartbeat;
pub mod plugin_state;
pub mod volume;
pub mod route;
