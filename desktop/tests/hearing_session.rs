use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant, SystemTime};

use audiocurve_app::model::app::{AppModel, Launch};
use audiocurve_app::model::hearing::{PauseReason, Phase, StopReason};
use audiocurve_app::platform::PlatformEvent;
use audiocurve_app::platform::fake::{FakeControl, fake};
use audiocurve_core::hearing_test::TestCalibration;
use audiocurve_core::heartbeat::InstanceHeartbeat;
use audiocurve_core::outputs::OutputDevice;
use audiocurve_core::route::RouteTarget;
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

/// Ticks past the route's coalesced apply.
fn settle(m: &mut AppModel, t: &mut Instant) {
    for _ in 0..3 {
        *t += Duration::from_millis(250);
        m.tick(*t);
    }
}

/// Opens the test with a fixed random source: no catch trials, 2.5 s gaps.
fn open(m: &mut AppModel, t: Instant) {
    m.open_hearing_test_with_random(Box::new(|| 0.5), t);
}

fn phase(m: &mut AppModel) -> Phase {
    m.hearing().expect("session open").phase().clone()
}

/// A fake listener: presses as soon as a tone at or above 35 dB HL starts.
struct Listener {
    seen: usize,
}

impl Listener {
    fn new(c: &FakeControl) -> Self {
        Listener {
            seen: c.tone_log().len(),
        }
    }

    fn react(&mut self, m: &mut AppModel, c: &FakeControl, t: Instant) {
        let log = c.tone_log();
        let new = log[self.seen..].to_vec();
        self.seen = log.len();
        for e in new {
            if played_hl(m, &e).is_some_and(|hl| hl >= 35.0 - 1e-6) {
                m.hearing_respond(t);
            }
        }
    }
}

/// The dB HL of a `play:<ear>:<hz>:<dbfs>` entry; None for anything else or a silent catch trial.
fn played_hl(m: &mut AppModel, entry: &str) -> Option<f64> {
    let rest = entry.strip_prefix("play:")?;
    let mut parts = rest.split(':');
    let _ear = parts.next()?;
    let f: f64 = parts.next()?.parse().ok()?;
    let dbfs: f64 = parts.next()?.parse().ok()?;
    let cal = TestCalibration::nominal(m.hearing()?.transducer());
    Some(cal.hl_from_dbfs(dbfs, f))
}

/// Advances the clock deadline by deadline, with the listener answering, until `done`.
fn drive(
    m: &mut AppModel,
    c: &FakeControl,
    t: &mut Instant,
    l: &mut Listener,
    mut done: impl FnMut(&mut AppModel, &FakeControl) -> bool,
) {
    for _ in 0..100_000 {
        if done(m, c) {
            return;
        }
        let d = m.next_deadline().expect("a deadline while testing");
        *t = d.max(*t);
        m.tick(*t);
        l.react(m, c, *t);
    }
    panic!("did not get there");
}

fn is_done(m: &mut AppModel, _c: &FakeControl) -> bool {
    matches!(phase(m), Phase::Done(_))
}

fn played_tones(c: &FakeControl) -> usize {
    c.tone_log()
        .iter()
        .filter(|e| e.starts_with("play:"))
        .count()
}

#[test]
fn full_run_saves_a_profile_assigned_to_the_output() {
    let (_d, s) = store();
    let (mut m, c, mut t) = start(&s);
    settle(&mut m, &mut t);
    open(&mut m, t);
    assert!(m.hearing().unwrap().can_start());
    m.hearing_start(t);
    assert!(m.testing);
    let mut l = Listener::new(&c);
    drive(&mut m, &c, &mut t, &mut l, is_done);
    assert!(!m.testing);
    let Phase::Done(result) = phase(&mut m) else {
        unreachable!()
    };
    assert_eq!(result.left.len(), 11);
    assert_eq!(result.right.len(), 11);
    let at_1k = result
        .left
        .iter()
        .find(|p| p.frequency_hz == 1000.0)
        .unwrap();
    assert_eq!(at_1k.hl, 35.0);

    m.hearing_save("  Quiet room  ", true);
    assert!(m.hearing().is_none(), "saving closes the session");
    let list = s.list();
    assert_eq!(list.len(), 1);
    let saved = s.saved(list[0].id).unwrap();
    assert_eq!(saved.name, "Quiet room");
    let record = saved.measurement.expect("the record is kept");
    assert_eq!(record.output.uid, "airpods");
    assert_eq!(record.left, result.left);
    assert_eq!(
        saved.profile.left,
        result.audiogram(audiocurve_core::profile::Ear::Left)
    );
    assert_eq!(saved.profile.left_trim_db, vec![0.0; 12]);
    assert_eq!(
        m.assignments
            .assignment("airpods")
            .and_then(|a| a.profile_id),
        Some(saved.id)
    );
    assert_eq!(m.editor.source_id, Some(saved.id));
}

#[test]
fn blank_name_saves_under_the_default_name_without_assigning() {
    let (_d, s) = store();
    let (mut m, c, mut t) = start(&s);
    settle(&mut m, &mut t);
    open(&mut m, t);
    m.hearing_start(t);
    let mut l = Listener::new(&c);
    drive(&mut m, &c, &mut t, &mut l, is_done);
    m.hearing_save("   ", false);
    let list = s.list();
    assert_eq!(list.len(), 1);
    assert!(
        list[0].name.starts_with("AirPods Pro – "),
        "{}",
        list[0].name
    );
    assert!(m.assignments.assignment("airpods").is_none());
}

#[test]
fn start_is_refused_when_muted_or_others_playing_or_other_host() {
    let (_d, s) = store();
    let (mut m, c, mut t) = start(&s);
    settle(&mut m, &mut t);

    c.set_muted("airpods", true);
    open(&mut m, t);
    assert!(m.hearing().unwrap().muted());
    assert!(!m.hearing().unwrap().can_start());
    m.hearing_start(t);
    assert_eq!(phase(&mut m), Phase::Intro);
    c.set_muted("airpods", false);
    t += Duration::from_secs(1);
    m.tick(t);
    assert!(
        !m.hearing().unwrap().muted(),
        "the 1 s check saw the unmute"
    );

    // Start re-reads the checks: a change since the last one-second check counts.
    c.set_others_playing(true);
    assert!(m.hearing().unwrap().can_start(), "not seen yet");
    m.hearing_start(t);
    assert_eq!(phase(&mut m), Phase::Intro);
    assert!(m.hearing().unwrap().others_playing());
    c.set_others_playing(false);
    t += Duration::from_secs(1);
    m.tick(t);
    assert!(m.hearing().unwrap().can_start(), "the 1 s check cleared it");

    s.write_heartbeat(&InstanceHeartbeat {
        pid: u32::MAX - 1,
        host_bundle_id: "com.example.mixer".into(),
        active: true,
        time: SystemTime::now(),
    });
    settle(&mut m, &mut t);
    settle(&mut m, &mut t);
    settle(&mut m, &mut t);
    assert_eq!(m.other_hosts, ["com.example.mixer"]);
    assert_eq!(
        m.hearing().unwrap().blocking_host(),
        Some("com.example.mixer")
    );
    assert!(!m.hearing().unwrap().can_start());
    m.hearing_start(t);
    assert_eq!(phase(&mut m), Phase::Intro);

    assert!(!m.testing);
    assert!(c.volume_log().is_empty());
    assert!(c.tone_log().is_empty());
}

#[test]
fn volume_is_saved_before_max_and_restored_after_finish() {
    let (_d, s) = store();
    let (mut m, c, mut t) = start(&s);
    settle(&mut m, &mut t);
    let pending_at_max: Rc<RefCell<Vec<Vec<VolumeSnapshot>>>> = Rc::default();
    {
        let s = s.clone();
        let seen = pending_at_max.clone();
        c.on_set_max(move |_| seen.borrow_mut().push(s.pending_volumes()));
    }
    open(&mut m, t);
    m.hearing_start(t);
    let at_max = pending_at_max.borrow().clone();
    assert_eq!(at_max.len(), 1);
    assert_eq!(at_max[0].len(), 1);
    assert_eq!(at_max[0][0].device_uid, "airpods");
    assert_eq!(at_max[0][0].scalars.get(&0), Some(&0.5));
    assert_eq!(m.hearing().unwrap().held_volume_uid(), Some("airpods"));

    let mut l = Listener::new(&c);
    drive(&mut m, &c, &mut t, &mut l, is_done);
    // The 2 s pending-volume retry ran during the test but left the held device alone.
    assert_eq!(c.volume_log(), ["max:airpods", "restore:airpods"]);
    assert!(s.pending_volumes().is_empty());
    assert_eq!(c.tone_log().last().map(String::as_str), Some("stop"));
    assert_eq!(m.hearing().unwrap().held_volume_uid(), None);
}

#[test]
fn route_is_paused_during_the_test_and_rebuilt_after() {
    let (_d, s) = store();
    let (mut m, c, mut t) = start(&s);
    settle(&mut m, &mut t);
    assert_eq!(c.route_log(), ["build:airpods"]);
    open(&mut m, t);
    m.hearing_start(t);
    let mut l = Listener::new(&c);
    // The route is torn down before the first tone plays.
    drive(&mut m, &c, &mut t, &mut l, |_, c| played_tones(c) > 0);
    assert_eq!(c.route_log(), ["build:airpods", "teardown:airpods"]);
    assert_eq!(
        m.route_state(),
        &RouteTarget::Paused {
            device_uid: "airpods".into()
        }
    );
    drive(&mut m, &c, &mut t, &mut l, is_done);
    assert!(!m.testing);
    settle(&mut m, &mut t);
    assert_eq!(
        c.route_log(),
        ["build:airpods", "teardown:airpods", "build:airpods"]
    );
    assert_eq!(
        m.route_state(),
        &RouteTarget::Running {
            device_uid: "airpods".into()
        }
    );
}

#[test]
fn others_playing_pauses_and_resume_rechecks() {
    let (_d, s) = store();
    let (mut m, c, mut t) = start(&s);
    settle(&mut m, &mut t);
    open(&mut m, t);
    m.hearing_start(t);
    let mut l = Listener::new(&c);
    drive(&mut m, &c, &mut t, &mut l, |_, c| played_tones(c) >= 2);

    c.set_others_playing(true);
    drive(&mut m, &c, &mut t, &mut l, |m, _| {
        matches!(phase(m), Phase::Paused(_))
    });
    assert_eq!(phase(&mut m), Phase::Paused(PauseReason::OtherSound));
    assert_eq!(c.tone_log().last().map(String::as_str), Some("stop"));
    assert_eq!(c.volume_log(), ["max:airpods", "restore:airpods"]);
    assert!(s.pending_volumes().is_empty());
    assert!(m.testing, "the route stays paused while the test is paused");
    assert!(!m.hearing().unwrap().can_resume());

    // Resume checks again: still playing, so it stays paused.
    m.hearing_resume(t);
    assert_eq!(phase(&mut m), Phase::Paused(PauseReason::OtherSound));
    assert_eq!(c.volume_log().len(), 2);

    // Resume reads the checks itself; it does not wait for the 1 s check.
    c.set_others_playing(false);
    m.hearing_resume(t);
    assert!(matches!(phase(&mut m), Phase::Running(_)));
    assert_eq!(
        c.volume_log(),
        ["max:airpods", "restore:airpods", "max:airpods"]
    );
    assert_eq!(
        c.tone_log()
            .iter()
            .filter(|e| e.starts_with("start:"))
            .count(),
        2
    );

    // A user pause works the same way, and a press while paused counts for nothing.
    m.hearing_pause(t);
    assert_eq!(phase(&mut m), Phase::Paused(PauseReason::User));
    assert_eq!(
        c.volume_log().last().map(String::as_str),
        Some("restore:airpods")
    );
    assert!(m.hearing().unwrap().can_resume());
    m.hearing_respond(t);
    m.hearing_resume(t);
    drive(&mut m, &c, &mut t, &mut l, is_done);
    assert_eq!(
        c.volume_log().last().map(String::as_str),
        Some("restore:airpods")
    );
}

#[test]
fn output_change_stops_the_test_and_restores_volume() {
    let (_d, s) = store();
    let (mut m, c, mut t) = start(&s);
    settle(&mut m, &mut t);
    open(&mut m, t);
    m.hearing_start(t);
    let mut l = Listener::new(&c);
    drive(&mut m, &c, &mut t, &mut l, |_, c| played_tones(c) >= 1);

    let outs = vec![
        OutputDevice {
            uid: "airpods".into(),
            name: "AirPods Pro".into(),
        },
        OutputDevice {
            uid: "speakers".into(),
            name: "Mac Studio Speakers".into(),
        },
    ];
    c.set_outputs(outs, Some("speakers"));
    c.emit(PlatformEvent::OutputsChanged);
    m.tick(t);
    let h = m.hearing().unwrap();
    assert_eq!(*h.phase(), Phase::Intro);
    assert_eq!(h.stop_reason(), Some(StopReason::OutputChanged));
    assert_eq!(c.tone_log().last().map(String::as_str), Some("stop"));
    assert_eq!(
        c.volume_log().last().map(String::as_str),
        Some("restore:airpods")
    );
    assert!(s.pending_volumes().is_empty());
    assert!(!m.testing);
}

#[test]
fn other_host_stops_the_test() {
    let (_d, s) = store();
    let (mut m, c, mut t) = start(&s);
    settle(&mut m, &mut t);
    open(&mut m, t);
    m.hearing_start(t);
    let mut l = Listener::new(&c);
    drive(&mut m, &c, &mut t, &mut l, |_, c| played_tones(c) >= 1);
    s.write_heartbeat(&InstanceHeartbeat {
        pid: u32::MAX - 1,
        host_bundle_id: "com.example.mixer".into(),
        active: true,
        time: SystemTime::now(),
    });
    drive(&mut m, &c, &mut t, &mut l, |m, _| phase(m) == Phase::Intro);
    assert_eq!(
        m.hearing().unwrap().stop_reason(),
        Some(StopReason::OtherHost)
    );
    assert_eq!(
        c.volume_log().last().map(String::as_str),
        Some("restore:airpods")
    );
}

#[test]
fn cancel_restores_volume_and_closes() {
    let (_d, s) = store();
    let (mut m, c, mut t) = start(&s);
    settle(&mut m, &mut t);
    open(&mut m, t);
    m.hearing_start(t);
    let mut l = Listener::new(&c);
    drive(&mut m, &c, &mut t, &mut l, |_, c| played_tones(c) >= 1);
    m.hearing_cancel(t);
    assert!(m.hearing().is_none());
    assert!(!m.testing);
    assert_eq!(c.tone_log().last().map(String::as_str), Some("stop"));
    assert_eq!(c.volume_log(), ["max:airpods", "restore:airpods"]);
    assert!(s.pending_volumes().is_empty());
    settle(&mut m, &mut t);
    assert_eq!(
        c.route_log().last().map(String::as_str),
        Some("build:airpods")
    );
}

#[test]
fn window_closed_stops_the_test() {
    let (_d, s) = store();
    let (mut m, c, mut t) = start(&s);
    settle(&mut m, &mut t);
    open(&mut m, t);
    m.hearing_start(t);
    m.hearing_window_closed(t);
    let h = m.hearing().unwrap();
    assert_eq!(*h.phase(), Phase::Intro);
    assert_eq!(h.stop_reason(), Some(StopReason::WindowClosed));
    assert_eq!(c.volume_log(), ["max:airpods", "restore:airpods"]);
}

#[test]
fn quit_stops_the_test_and_restores_volume() {
    let (_d, s) = store();
    let (mut m, c, mut t) = start(&s);
    settle(&mut m, &mut t);
    open(&mut m, t);
    m.hearing_start(t);
    m.quit();
    assert_eq!(c.tone_log().last().map(String::as_str), Some("stop"));
    assert_eq!(c.volume_log(), ["max:airpods", "restore:airpods"]);
    assert!(s.pending_volumes().is_empty());
}

#[test]
fn failed_restore_keeps_the_pending_snapshot() {
    let (_d, s) = store();
    let (mut m, c, mut t) = start(&s);
    settle(&mut m, &mut t);
    open(&mut m, t);
    m.hearing_start(t);
    c.fail_next_restores(1);
    m.hearing_cancel(t);
    assert_eq!(c.volume_log(), ["max:airpods", "restore:airpods"]);
    let pending = s.pending_volumes();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].device_uid, "airpods");
    assert_eq!(pending[0].scalars.get(&0), Some(&0.5));
    assert_eq!(pending[0].failed_attempts, 1);
}

#[test]
fn snapshot_left_by_a_failed_restore_is_retried_every_2_s() {
    let (_d, s) = store();
    let (mut m, c, mut t) = start(&s);
    settle(&mut m, &mut t);
    open(&mut m, t);
    m.hearing_start(t);
    c.fail_next_restores(2);
    m.hearing_cancel(t);
    assert_eq!(s.pending_volumes().len(), 1);
    // The next retry fails too; the one after succeeds.
    let end = t + Duration::from_secs(5);
    while t < end {
        t = m.next_deadline().unwrap().max(t);
        m.tick(t);
    }
    assert_eq!(
        c.volume_log(),
        [
            "max:airpods",
            "restore:airpods",
            "restore:airpods",
            "restore:airpods"
        ]
    );
    assert!(s.pending_volumes().is_empty());
}

#[test]
fn a_pending_snapshot_is_adopted_not_replaced_when_raising_again() {
    let (_d, s) = store();
    let (mut m, c, mut t) = start(&s);
    settle(&mut m, &mut t);
    open(&mut m, t);
    m.hearing_start(t);
    let mut l = Listener::new(&c);
    drive(&mut m, &c, &mut t, &mut l, |_, c| played_tones(c) >= 1);
    c.fail_next_restores(1);
    m.hearing_pause(t);
    assert_eq!(s.pending_volumes().len(), 1);
    // The pause's restore failed; resume adopts the saved original instead of re-reading.
    m.hearing_resume(t);
    assert_eq!(m.hearing().unwrap().held_volume_uid(), Some("airpods"));
    assert_eq!(s.pending_volumes()[0].failed_attempts, 1);
    m.hearing_cancel(t);
    assert!(s.pending_volumes().is_empty());
}

/// A saved "Kitchen" version loaded and then edited without saving.
fn dirty_kitchen(
    m: &mut AppModel,
    s: &ProfileStore,
    t: Instant,
) -> (uuid::Uuid, audiocurve_core::profile::HearingProfile) {
    let base = audiocurve_core::profile::HearingProfile::flat();
    let kitchen = s.save_new("Kitchen", &base).unwrap().id;
    m.load_profile(kitchen, t);
    let mut edited = base.clone();
    edited.left_trim_db[3] = 4.0;
    m.editor.set_profile(edited.clone(), t);
    assert!(m.editor.is_dirty());
    (kitchen, edited)
}

#[test]
fn saving_with_use_on_saves_unsaved_edits_and_loads_the_result() {
    let (_d, s) = store();
    let (mut m, c, mut t) = start(&s);
    settle(&mut m, &mut t);
    let (kitchen, edited) = dirty_kitchen(&mut m, &s, t);
    open(&mut m, t);
    m.hearing_start(t);
    let mut l = Listener::new(&c);
    drive(&mut m, &c, &mut t, &mut l, is_done);
    m.hearing_save("Quiet room", true);
    assert_eq!(
        s.saved(kitchen).unwrap().profile,
        edited,
        "the edits were saved into their version first"
    );
    let new = s
        .list()
        .into_iter()
        .find(|p| p.name == "Quiet room")
        .unwrap();
    assert_eq!(m.editor.source_id, Some(new.id));
    assert_eq!(m.editor.profile, Some(new.profile));
}

#[test]
fn saving_without_use_on_keeps_the_loaded_profile_and_its_edits() {
    let (_d, s) = store();
    let (mut m, c, mut t) = start(&s);
    settle(&mut m, &mut t);
    let (kitchen, edited) = dirty_kitchen(&mut m, &s, t);
    open(&mut m, t);
    m.hearing_start(t);
    let mut l = Listener::new(&c);
    drive(&mut m, &c, &mut t, &mut l, is_done);
    m.hearing_save("Quiet room", false);
    assert!(s.list().iter().any(|p| p.name == "Quiet room"));
    assert!(
        m.editor.saved.iter().any(|p| p.name == "Quiet room"),
        "the saved list is refreshed"
    );
    assert_eq!(m.editor.source_id, Some(kitchen));
    assert_eq!(m.editor.profile, Some(edited));
}

#[test]
fn saving_with_use_on_for_another_output_keeps_the_loaded_profile() {
    let (_d, s) = store();
    let (mut m, c, mut t) = start(&s);
    settle(&mut m, &mut t);
    dirty_kitchen(&mut m, &s, t);
    open(&mut m, t);
    m.hearing_start(t);
    let mut l = Listener::new(&c);
    drive(&mut m, &c, &mut t, &mut l, is_done);
    // The output changed after the test finished: the result belongs to the AirPods.
    let outs = vec![
        OutputDevice {
            uid: "airpods".into(),
            name: "AirPods Pro".into(),
        },
        OutputDevice {
            uid: "speakers".into(),
            name: "Mac Studio Speakers".into(),
        },
    ];
    c.set_outputs(outs, Some("speakers"));
    c.emit(PlatformEvent::OutputsChanged);
    m.tick(t);
    let before = (m.editor.source_id, m.editor.profile.clone());
    m.hearing_save("Quiet room", true);
    let new = s
        .list()
        .into_iter()
        .find(|p| p.name == "Quiet room")
        .unwrap();
    assert_eq!(
        m.assignments
            .assignment("airpods")
            .and_then(|a| a.profile_id),
        Some(new.id)
    );
    assert_eq!((m.editor.source_id, m.editor.profile.clone()), before);
}

// MARK: Safety paths the default fake does not reach

/// The index of the last entry equal to `e` in the ordered tone and volume log.
fn last(c: &FakeControl, e: &str) -> usize {
    c.event_log()
        .iter()
        .rposition(|x| x == e)
        .unwrap_or_else(|| panic!("no {e} in {:?}", c.event_log()))
}

/// Tones went off before the volume came back, and nothing was raised after.
fn assert_stopped_then_restored(c: &FakeControl) {
    let log = c.event_log();
    let (stop, restore) = (last(c, "tone:stop"), last(c, "volume:restore:airpods"));
    assert!(stop < restore, "{log:?}");
    assert!(
        !log[stop..].iter().any(|e| e.starts_with("tone:play")
            || e.starts_with("tone:start")
            || e.starts_with("volume:max")),
        "{log:?}"
    );
}

#[test]
fn volume_not_set_restores_and_stops() {
    let (_d, s) = store();
    let (mut m, c, mut t) = start(&s);
    settle(&mut m, &mut t);
    c.fail_set_max(true);
    open(&mut m, t);
    m.hearing_start(t);
    let h = m.hearing().unwrap();
    assert_eq!(*h.phase(), Phase::Intro);
    assert_eq!(h.stop_reason(), Some(StopReason::VolumeNotSet));
    assert_eq!(h.held_volume_uid(), None);
    assert_eq!(c.volume_log(), ["max:airpods", "restore:airpods"]);
    assert!(last(&c, "volume:max:airpods") < last(&c, "volume:restore:airpods"));
    assert_eq!(c.tone_log().last().map(String::as_str), Some("stop"));
    assert_eq!(played_tones(&c), 0);
    assert!(s.pending_volumes().is_empty());
    assert!(!m.testing);
    // `testing` went on and off within the call, so the coalesced route never paused.
    settle(&mut m, &mut t);
    assert_eq!(
        m.route_state(),
        &RouteTarget::Running {
            device_uid: "airpods".into()
        }
    );
    assert_eq!(
        c.route_log().last().map(String::as_str),
        Some("build:airpods")
    );
}

#[test]
fn tones_stop_before_the_volume_is_restored_on_pause() {
    let (_d, s) = store();
    let (mut m, c, mut t) = start(&s);
    settle(&mut m, &mut t);
    open(&mut m, t);
    m.hearing_start(t);
    let mut l = Listener::new(&c);
    drive(&mut m, &c, &mut t, &mut l, |_, c| played_tones(c) >= 1);
    m.hearing_pause(t);
    assert_stopped_then_restored(&c);
}

#[test]
fn tones_stop_before_the_volume_is_restored_on_finish() {
    let (_d, s) = store();
    let (mut m, c, mut t) = start(&s);
    settle(&mut m, &mut t);
    open(&mut m, t);
    m.hearing_start(t);
    let mut l = Listener::new(&c);
    drive(&mut m, &c, &mut t, &mut l, is_done);
    assert_stopped_then_restored(&c);
}

#[test]
fn tones_stop_before_the_volume_is_restored_on_cancel() {
    let (_d, s) = store();
    let (mut m, c, mut t) = start(&s);
    settle(&mut m, &mut t);
    open(&mut m, t);
    m.hearing_start(t);
    let mut l = Listener::new(&c);
    drive(&mut m, &c, &mut t, &mut l, |_, c| played_tones(c) >= 1);
    m.hearing_cancel(t);
    assert_stopped_then_restored(&c);
}

#[test]
fn tones_stop_before_the_volume_is_restored_on_output_change() {
    let (_d, s) = store();
    let (mut m, c, mut t) = start(&s);
    settle(&mut m, &mut t);
    open(&mut m, t);
    m.hearing_start(t);
    let mut l = Listener::new(&c);
    drive(&mut m, &c, &mut t, &mut l, |_, c| played_tones(c) >= 1);
    let outs = vec![
        OutputDevice {
            uid: "airpods".into(),
            name: "AirPods Pro".into(),
        },
        OutputDevice {
            uid: "speakers".into(),
            name: "Mac Studio Speakers".into(),
        },
    ];
    c.set_outputs(outs, Some("speakers"));
    c.emit(PlatformEvent::OutputsChanged);
    m.tick(t);
    assert_eq!(
        m.hearing().unwrap().stop_reason(),
        Some(StopReason::OutputChanged)
    );
    assert_stopped_then_restored(&c);
}

#[test]
fn generator_that_fails_to_start_stops_the_test_before_raising() {
    let (_d, s) = store();
    let (mut m, c, mut t) = start(&s);
    settle(&mut m, &mut t);
    c.fail_next_tone_starts(1);
    open(&mut m, t);
    m.hearing_start(t);
    let h = m.hearing().unwrap();
    assert_eq!(*h.phase(), Phase::Intro);
    assert_eq!(h.stop_reason(), Some(StopReason::GeneratorFailed));
    assert!(c.volume_log().is_empty(), "the volume was never raised");
    assert!(s.pending_volumes().is_empty());
    assert!(!m.testing);
}

#[test]
fn generator_that_stops_mid_run_stops_the_test() {
    let (_d, s) = store();
    let (mut m, c, mut t) = start(&s);
    settle(&mut m, &mut t);
    open(&mut m, t);
    m.hearing_start(t);
    let mut l = Listener::new(&c);
    drive(&mut m, &c, &mut t, &mut l, |_, c| played_tones(c) >= 1);
    c.stop_generator();
    drive(&mut m, &c, &mut t, &mut l, |m, _| phase(m) == Phase::Intro);
    assert_eq!(
        m.hearing().unwrap().stop_reason(),
        Some(StopReason::GeneratorStopped)
    );
    assert_stopped_then_restored(&c);
    assert!(s.pending_volumes().is_empty());
    assert!(!m.testing);
}

#[test]
fn low_sample_rate_refuses_start_and_stops_a_running_test() {
    let (_d, s) = store();
    let (mut m, c, mut t) = start(&s);
    settle(&mut m, &mut t);
    c.set_sample_rate(22_050.0);
    open(&mut m, t);
    assert!(m.hearing().unwrap().low_sample_rate());
    assert!(!m.hearing().unwrap().can_start());
    m.hearing_start(t);
    assert_eq!(phase(&mut m), Phase::Intro);
    assert!(c.volume_log().is_empty());
    assert!(c.tone_log().is_empty());
    assert!(!m.testing);

    c.set_sample_rate(48_000.0);
    t += Duration::from_secs(1);
    m.tick(t);
    assert!(m.hearing().unwrap().can_start());
    m.hearing_start(t);
    let mut l = Listener::new(&c);
    drive(&mut m, &c, &mut t, &mut l, |_, c| played_tones(c) >= 1);
    c.set_sample_rate(22_050.0);
    drive(&mut m, &c, &mut t, &mut l, |m, _| phase(m) == Phase::Intro);
    assert_eq!(
        m.hearing().unwrap().stop_reason(),
        Some(StopReason::LowSampleRate)
    );
    assert_stopped_then_restored(&c);
    assert!(!m.testing);
}

#[test]
fn manual_volume_output_runs_without_raising_and_cancel_returns_to_intro() {
    let (_d, s) = store();
    let (mut m, c, mut t) = start(&s);
    settle(&mut m, &mut t);
    c.set_volume_settable(false);
    open(&mut m, t);
    assert!(!m.hearing().unwrap().volume_settable());
    assert!(m.hearing().unwrap().can_start());
    m.hearing_start(t);
    let mut l = Listener::new(&c);
    drive(&mut m, &c, &mut t, &mut l, |_, c| played_tones(c) >= 1);
    assert!(matches!(phase(&mut m), Phase::Running(_)));
    assert!(c.volume_log().is_empty(), "AudioCurve cannot set it");
    assert!(s.pending_volumes().is_empty());
    assert!(!m.hearing().unwrap().remind_volume_down());

    m.hearing_cancel(t);
    let h = m
        .hearing()
        .expect("cancel goes back to the intro, not closed");
    assert_eq!(*h.phase(), Phase::Intro);
    assert_eq!(h.stop_reason(), None);
    assert!(h.remind_volume_down());
    assert_eq!(c.tone_log().last().map(String::as_str), Some("stop"));
    assert!(!m.testing);
    // On the intro, Cancel closes.
    m.hearing_cancel(t);
    assert!(m.hearing().is_none());
}
