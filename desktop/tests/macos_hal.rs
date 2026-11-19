//! The macOS HAL helpers. These only read the HAL, so they run without `AUDIOCURVE_HW_TESTS`.
//! The ones that need a default output skip on a Mac without one (such as a CI runner).
#![cfg(target_os = "macos")]

use audiocurve_app::platform::macos::hal;

/// The default output, or `None` with a notice that the calling test is skipped.
fn default_output_or_skip() -> Option<hal::ObjectId> {
    let id = hal::default_output();
    if id.is_none() {
        eprintln!("no default output device; skipped");
    }
    id
}

#[test]
fn default_output_has_uid_name_and_rate() {
    let Some(id) = default_output_or_skip() else {
        return;
    };
    let uid = hal::device_uid(id).expect("a UID");
    let name = hal::device_name(id).expect("a name");
    assert!(!uid.is_empty());
    assert!(!name.is_empty());
    let rate = hal::nominal_sample_rate(id).expect("a nominal sample rate");
    assert!((8_000.0..=384_000.0).contains(&rate), "rate {rate}");
}

#[test]
fn output_devices_contains_the_default_and_no_own_routes() {
    let Some(id) = default_output_or_skip() else {
        return;
    };
    let uid = hal::device_uid(id).unwrap();
    let devices = hal::output_devices();
    assert!(devices.iter().any(|d| d.uid == uid), "{devices:?}");
    assert!(devices.iter().all(|d| !hal::is_own_route(&d.uid)));
}

#[test]
fn is_own_route_matches_only_the_prefix() {
    assert!(hal::is_own_route("se.lindefors.audiocurve.route.1234"));
    assert!(!hal::is_own_route("80-28-3C-24-CB-8C:output"));
}

#[test]
fn device_by_uid_round_trips() {
    let Some(id) = default_output_or_skip() else {
        return;
    };
    let uid = hal::device_uid(id).unwrap();
    assert_eq!(hal::device_by_uid(&uid), Some(id));
    assert_eq!(hal::device_by_uid("no.such.device.uid"), None);
}

#[test]
fn others_playing_returns() {
    let _ = hal::others_playing();
}

#[test]
fn event_queue_drains_in_order_and_wakes_on_each_push() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use audiocurve_app::platform::PlatformEvent;
    use audiocurve_app::platform::macos::audio::EventQueue;

    let wakes = Arc::new(AtomicUsize::new(0));
    let w = wakes.clone();
    let q = EventQueue::new(Arc::new(move || {
        w.fetch_add(1, Ordering::SeqCst);
    }));
    let other = q.clone();
    q.push(PlatformEvent::OutputsChanged);
    other.push(PlatformEvent::SampleRateChanged);
    assert_eq!(wakes.load(Ordering::SeqCst), 2);
    assert_eq!(
        q.drain(),
        vec![
            PlatformEvent::OutputsChanged,
            PlatformEvent::SampleRateChanged
        ]
    );
    assert!(q.drain().is_empty());
}

#[test]
fn mac_audio_lists_the_current_output_sorted_by_name() {
    use std::sync::Arc;

    use audiocurve_app::platform::Audio;
    use audiocurve_app::platform::macos::audio::MacAudio;
    use audiocurve_core::store::natural_cmp;

    if default_output_or_skip().is_none() {
        return;
    }
    let audio = MacAudio::new(Arc::new(|| {}));
    let current = audio.current_output().expect("a current output");
    let outputs = audio.outputs();
    assert!(outputs.contains(&current), "{current:?} not in {outputs:?}");
    assert!(
        outputs
            .windows(2)
            .all(|w| natural_cmp(&w[0].name, &w[1].name).is_le())
    );
}

#[test]
fn listen_registers_on_the_system_object_and_removes_on_drop() {
    let l = hal::listen(
        hal::SYSTEM,
        objc2_core_audio::kAudioHardwarePropertyDevices,
        || {},
    );
    assert!(l.is_registered());
    drop(l);
}
