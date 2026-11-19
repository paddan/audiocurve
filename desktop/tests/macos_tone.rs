//! The hearing test tone output. The pure tests always run; the hardware test starts the
//! generator on the default output and plays only a silent catch-trial tone, and only with
//! `AUDIOCURVE_HW_TESTS=1`.
#![cfg(target_os = "macos")]

use std::time::{Duration, Instant};

use audiocurve_app::platform::TestTone;
use audiocurve_app::platform::macos::hal;
use audiocurve_app::platform::macos::tone::{MacTestTone, check_rate};
use audiocurve_core::profile::Ear;

fn enabled() -> bool {
    std::env::var("AUDIOCURVE_HW_TESTS").as_deref() == Ok("1")
}

#[test]
fn tone_rejects_low_sample_rate() {
    assert!(check_rate(16_000.0).is_err());
    assert!(check_rate(22_049.0).is_err());
    assert!(check_rate(f64::NAN).is_err());
    assert!(check_rate(f64::INFINITY).is_err());
    assert!(check_rate(22_050.0).is_ok());
    assert!(check_rate(44_100.0).is_ok());
    assert!(check_rate(48_000.0).is_ok());
}

#[test]
fn start_fails_for_an_unknown_device() {
    let mut tone = MacTestTone::new();
    assert!(tone.start("no-such-device-uid").is_err());
    assert!(!tone.is_playing());
    assert_eq!(tone.sample_rate("no-such-device-uid"), 0.0);
}

/// Stops the generator on drop, even when an assertion fails.
struct Guard(MacTestTone);

impl Drop for Guard {
    fn drop(&mut self) {
        self.0.stop();
    }
}

#[test]
fn start_play_silent_and_stop_on_default_output() {
    if !enabled() {
        return;
    }
    let uid = hal::device_uid(hal::default_output().expect("a default output")).expect("a UID");
    let rate = MacTestTone::new().sample_rate(&uid);
    if check_rate(rate).is_err() {
        eprintln!("default output runs at {rate} Hz, below the minimum; nothing to test");
        return;
    }
    let mut g = Guard(MacTestTone::new());
    g.0.start(&uid).expect("start");
    assert!(g.0.is_playing());

    // Silent catch trial only: never an audible tone from a test.
    g.0.play(Ear::Left, 1000.0, None);
    assert!(g.0.is_tone_sounding());
    std::thread::sleep(Duration::from_millis(1200));
    assert!(g.0.is_playing(), "still running after the tone");
    // A Bluetooth output takes ~1.5 s before its first IO callback.
    let deadline = Instant::now() + Duration::from_secs(3);
    while g.0.is_tone_sounding() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(
        !g.0.is_tone_sounding(),
        "the IO thread rendered the whole tone"
    );

    g.0.stop();
    assert!(!g.0.is_playing());
    assert!(!g.0.is_tone_sounding());
}
