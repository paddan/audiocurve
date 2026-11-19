//! The macOS platform assembles and lists outputs. It builds no route and touches no volume.
//! It skips on a Mac without a default output (such as a CI runner).
#![cfg(target_os = "macos")]

use std::sync::Arc;

use audiocurve_app::platform::macos::hal;
use audiocurve_app::platform::macos::mac_platform;
use audiocurve_app::platform::macos::tray::NoTray;

#[test]
fn mac_platform_assembles_and_lists_outputs() {
    if hal::default_output().is_none() {
        eprintln!("no default output device; skipped");
        return;
    }
    let platform = mac_platform(Arc::new(|| {}), Box::new(NoTray));
    assert!(!platform.audio.outputs().is_empty());
    assert!(platform.audio.current_output().is_some());
}
