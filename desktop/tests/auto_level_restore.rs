use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use audiocurve_app::model::app::{AppModel, Launch};
use audiocurve_app::model::auto_level::AutoLevelUi;
use audiocurve_app::platform::PlatformEvent;
use audiocurve_app::platform::fake::{FakeControl, fake};
use audiocurve_core::outputs::OutputDevice;
use audiocurve_core::store::ProfileStore;
use audiocurve_core::volume::VolumeSnapshot;

fn store() -> (tempfile::TempDir, ProfileStore) {
    let dir = tempfile::tempdir().unwrap();
    let s = ProfileStore::new(dir.path().to_path_buf());
    (dir, s)
}

fn start(store: &ProfileStore) -> (AppModel, FakeControl, Instant) {
    let (p, c) = fake();
    let t = Instant::now();
    let m = AppModel::new(
        p,
        store.clone(),
        Launch {
            from_login_item: false,
        },
        t,
    );
    (m, c, t)
}

fn snapshot(uid: &str) -> VolumeSnapshot {
    VolumeSnapshot {
        device_uid: uid.into(),
        scalars: BTreeMap::from([(0, 0.4)]),
        failed_attempts: 0,
    }
}

/// Ticks every 50 ms, pumping 50 ms of audio before each, until the measurement ends.
fn run_measurement(
    m: &mut AppModel,
    c: &FakeControl,
    t: &mut Instant,
    pump: bool,
) -> Vec<AutoLevelUi> {
    let mut seen = vec![*m.auto_level()];
    for _ in 0..200 {
        if pump {
            c.pump(2400);
        }
        m.tick(*t);
        if seen.last() != Some(m.auto_level()) {
            seen.push(*m.auto_level());
        }
        if !matches!(m.auto_level(), AutoLevelUi::Measuring { .. }) {
            break;
        }
        *t += Duration::from_millis(50);
    }
    seen
}

#[test]
fn auto_level_measures_60_blocks_then_sets_output_gain() {
    let (_d, s) = store();
    let (mut m, c, mut t) = start(&s);
    m.tick(t); // builds the route
    assert_eq!(*m.auto_level(), AutoLevelUi::Idle);
    m.levels.output_gain_db = 6.0;
    m.start_auto_level(t);
    assert_eq!(*m.auto_level(), AutoLevelUi::Measuring { remaining: 60 });
    assert_eq!(m.next_deadline().map(|d| d <= t), Some(true));
    let seen = run_measurement(&mut m, &c, &mut t, true);
    assert_eq!(seen.len(), 61, "{seen:?}"); // Measuring 60 down to 1, then Done
    assert_eq!(seen[0], AutoLevelUi::Measuring { remaining: 60 });
    assert_eq!(seen[1], AutoLevelUi::Measuring { remaining: 59 });
    let AutoLevelUi::Done(g) = *seen.last().unwrap() else {
        panic!("{seen:?}")
    };
    assert_ne!(g, 6.0, "the gain was measured, not left as it was");
    assert_eq!(m.levels.output_gain_db, g);
    assert_eq!(g, (g * 10.0).round() / 10.0);
}

#[test]
fn auto_level_with_silence_is_too_quiet() {
    let (_d, s) = store();
    let (mut m, c, mut t) = start(&s);
    m.tick(t);
    m.start_auto_level(t);
    run_measurement(&mut m, &c, &mut t, false);
    assert_eq!(*m.auto_level(), AutoLevelUi::TooQuiet);
    assert_eq!(m.levels.output_gain_db, 0.0);
}

#[test]
fn pending_volume_is_restored_at_start_and_cleared() {
    let (_d, s) = store();
    s.save_pending_volume(snapshot("airpods"));
    let (m, c, _t) = start(&s);
    assert_eq!(c.volume_log(), vec!["restore:airpods".to_string()]);
    assert!(s.pending_volumes().is_empty());
    drop(m);
}

#[test]
fn pending_volume_of_absent_device_waits_until_it_returns() {
    let (_d, s) = store();
    s.save_pending_volume(snapshot("usb"));
    let (mut m, c, mut t) = start(&s);
    assert!(c.volume_log().is_empty());
    assert_eq!(s.pending_volumes().len(), 1);
    // Ticking alone does not restore it.
    t += Duration::from_secs(3);
    m.tick(t);
    assert!(c.volume_log().is_empty());
    let outs = vec![
        OutputDevice {
            uid: "airpods".into(),
            name: "AirPods Pro".into(),
        },
        OutputDevice {
            uid: "usb".into(),
            name: "USB".into(),
        },
    ];
    c.set_outputs(outs, Some("airpods"));
    c.emit(PlatformEvent::OutputsChanged);
    m.tick(t);
    assert_eq!(c.volume_log(), vec!["restore:usb".to_string()]);
    assert!(s.pending_volumes().is_empty());
}
