mod support;

use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use audiocurve_app::i18n::{self, formatted_date_time};
use audiocurve_app::model::app::{AppModel, Launch};
use audiocurve_app::model::settings::Settings;
use audiocurve_app::model::status::{Tone, status_text};
use audiocurve_app::platform::PlatformEvent;
use audiocurve_app::platform::fake::{FakeControl, fake};
use audiocurve_core::heartbeat::InstanceHeartbeat;
use audiocurve_core::outputs::{LevelSettings, OutputAssignments, OutputDevice};
use audiocurve_core::profile::HearingProfile;
use audiocurve_core::route::{RouteFailure, RouteTarget};
use audiocurve_core::store::ProfileStore;
use support::example_profile;

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

/// Ticks at each deadline until there is none or 5 s have passed.
fn settle(m: &mut AppModel, t: &mut Instant) {
    let end = *t + Duration::from_secs(5);
    m.tick(*t);
    for _ in 0..1000 {
        match m.next_deadline() {
            Some(d) if d <= end => {
                *t = d.max(*t);
                m.tick(*t);
            }
            _ => break,
        }
    }
}

fn device(uid: &str, name: &str) -> OutputDevice {
    OutputDevice {
        uid: uid.into(),
        name: name.into(),
    }
}

fn airpods() -> OutputDevice {
    device("airpods", "AirPods Pro")
}

fn aids() -> OutputDevice {
    device("aids", "Hearing Aids")
}

fn switch_to(c: &FakeControl, uid: &str) {
    let outs = vec![airpods(), aids(), device("speakers", "Mac Studio Speakers")];
    c.set_outputs(outs, Some(uid));
    c.emit(PlatformEvent::OutputsChanged);
}

fn edited(p: &HearingProfile) -> HearingProfile {
    let mut p = p.clone();
    p.left_trim_db[3] = 4.0;
    p
}

fn running(uid: &str) -> RouteTarget {
    RouteTarget::Running {
        device_uid: uid.into(),
    }
}

#[test]
fn starts_running_on_the_current_output() {
    let (_d, s) = store();
    let (mut m, c, mut t) = start(&s);
    settle(&mut m, &mut t);
    assert_eq!(c.route_log(), ["build:airpods"]);
    assert_eq!(m.route_state(), &running("airpods"));
    assert_eq!(
        status_text(&m),
        ("Active – AirPods Pro".to_string(), Tone::Positive)
    );
    assert!(m.window_open);
    assert!(!m.bypass);
    assert!(m.processing_active());
}

#[test]
fn foreground_mode_starts_bypassed() {
    let (_d, s) = store();
    Settings {
        background_processing: false,
        ..Settings::default()
    }
    .save(s.root());
    let (mut m, c, mut t) = start(&s);
    settle(&mut m, &mut t);
    assert!(m.bypass);
    assert!(!m.processing_active());
    assert_eq!(c.route_log(), ["build:airpods"]);

    m.set_bypass(false, t);
    assert!(m.processing_active());
}

#[test]
fn turning_off_tears_down() {
    let (_d, s) = store();
    let (mut m, c, mut t) = start(&s);
    settle(&mut m, &mut t);
    m.set_enabled(false, t);
    settle(&mut m, &mut t);
    assert_eq!(c.route_log(), ["build:airpods", "teardown:airpods"]);
    assert_eq!(m.route_state(), &RouteTarget::Off);
    assert_eq!(status_text(&m), ("Off".to_string(), Tone::Neutral));
    assert!(!m.processing_active());
    assert!(!Settings::load(s.root()).enabled);
}

#[test]
fn unticked_output_pauses_and_says_so() {
    let (_d, s) = store();
    let (mut m, c, mut t) = start(&s);
    settle(&mut m, &mut t);
    m.set_output_enabled(&airpods(), false, t);
    settle(&mut m, &mut t);
    assert_eq!(c.route_log(), ["build:airpods", "teardown:airpods"]);
    assert_eq!(
        m.route_state(),
        &RouteTarget::Paused {
            device_uid: "airpods".into()
        }
    );
    assert_eq!(
        status_text(&m),
        (
            "Paused – AirPods Pro is the output".to_string(),
            Tone::Warning
        )
    );
    assert!(!m.active_outputs.is_checked("airpods"));
    assert_eq!(s.load_active_outputs(), m.active_outputs);
}

#[test]
fn output_change_loads_the_assigned_profile_and_levels() {
    let (_d, s) = store();
    let b = s.save_new("B", &example_profile()).unwrap();
    let levels = LevelSettings {
        strength: 80.0,
        input_gain_db: -3.0,
        output_gain_db: 2.0,
        tone: 10.0,
        mpo_dbfs: -6.0,
    };
    let mut a = OutputAssignments::default();
    a.assign(&aids(), Some(b.id), Some(levels));
    s.save_assignments(&a);

    let (mut m, c, mut t) = start(&s);
    settle(&mut m, &mut t);
    assert_eq!(m.editor.source_id, None);

    switch_to(&c, "aids");
    settle(&mut m, &mut t);
    assert_eq!(m.editor.source_id, Some(b.id));
    assert_eq!(m.editor.profile.as_ref(), Some(&b.profile));
    assert_eq!(m.levels.to_settings(), levels);
    assert_eq!(m.current_output, Some(aids()));
    assert_eq!(
        c.route_log(),
        ["build:airpods", "teardown:airpods", "build:aids"]
    );
    assert_eq!(
        status_text(&m),
        ("Active – Hearing Aids".to_string(), Tone::Positive)
    );
}

#[test]
fn output_switch_saves_unsaved_edits_first() {
    let (_d, s) = store();
    let a = s.save_new("A", &example_profile()).unwrap();
    let b = s.save_new("B", &HearingProfile::flat()).unwrap();
    let mut asg = OutputAssignments::default();
    asg.assign(&aids(), Some(b.id), None);
    s.save_assignments(&asg);

    let (mut m, c, mut t) = start(&s);
    m.load_profile(a.id, t);
    let changed = edited(&a.profile);
    m.editor.set_profile(changed.clone(), t);
    assert!(m.editor.is_dirty());

    switch_to(&c, "aids");
    settle(&mut m, &mut t);
    assert_eq!(s.saved(a.id).unwrap().profile, changed);
    assert_eq!(m.editor.source_id, Some(b.id));
    assert_eq!(m.editor.profile.as_ref(), Some(&b.profile));
    assert!(!m.editor.is_dirty());
}

#[test]
fn levels_on_an_assigned_output_are_saved_after_half_a_second() {
    let (_d, s) = store();
    let p = s.save_new("P", &example_profile()).unwrap();
    let (mut m, _c, mut t) = start(&s);
    settle(&mut m, &mut t);
    m.assign(&airpods(), Some(p.id));
    assert_eq!(
        s.load_assignments()
            .assignment("airpods")
            .unwrap()
            .profile_id,
        Some(p.id)
    );

    m.levels.strength = 75.0;
    m.tick(t);
    m.tick(t + Duration::from_millis(400));
    let saved = |s: &ProfileStore| {
        s.load_assignments()
            .assignment("airpods")
            .unwrap()
            .levels
            .unwrap()
            .strength
    };
    assert_eq!(saved(&s), 50.0);
    m.tick(t + Duration::from_millis(500));
    assert_eq!(saved(&s), 75.0);
    assert_eq!(
        m.assignments
            .assignment("airpods")
            .unwrap()
            .levels
            .unwrap()
            .strength,
        75.0
    );
}

#[test]
fn render_failure_rebuilds_engine_and_route() {
    let (_d, s) = store();
    let (mut m, c, mut t) = start(&s);
    settle(&mut m, &mut t);
    let before = m.engine();
    c.emit(PlatformEvent::RenderFailed {
        route_id: c.running_route_id().unwrap(),
    });
    settle(&mut m, &mut t);
    assert_eq!(
        c.route_log(),
        ["build:airpods", "teardown:airpods", "build:airpods"]
    );
    assert!(!Arc::ptr_eq(&before, &m.engine()));
    assert_eq!(m.route_state(), &running("airpods"));
}

#[test]
fn second_engine_failure_within_10s_latches_and_try_again_recovers() {
    let (_d, s) = store();
    let (mut m, c, mut t) = start(&s);
    settle(&mut m, &mut t);
    c.emit(PlatformEvent::RenderFailed {
        route_id: c.running_route_id().unwrap(),
    });
    settle(&mut m, &mut t);
    c.emit(PlatformEvent::RenderFailed {
        route_id: c.running_route_id().unwrap(),
    });
    settle(&mut m, &mut t);
    assert_eq!(m.route_state(), &RouteTarget::Failed(RouteFailure::Plugin));
    assert_eq!(
        status_text(&m),
        ("The sound processing stopped".to_string(), Tone::Warning)
    );

    // An output change does not leave the latch.
    switch_to(&c, "aids");
    settle(&mut m, &mut t);
    assert_eq!(m.route_state(), &RouteTarget::Failed(RouteFailure::Plugin));

    let before = m.engine();
    m.retry_engine(t);
    settle(&mut m, &mut t);
    assert_eq!(m.route_state(), &running("aids"));
    assert!(!Arc::ptr_eq(&before, &m.engine()));
}

/// A mid-run IO stall (as around sleep and wake) rebuilds the route but does not count as an
/// engine death: two within 10 s keep processing on, with the same engine.
#[test]
fn two_route_stalls_within_10s_rebuild_without_latching() {
    let (_d, s) = store();
    let (mut m, c, mut t) = start(&s);
    settle(&mut m, &mut t);
    let before = m.engine();
    let t0 = t;
    for _ in 0..2 {
        c.emit(PlatformEvent::RouteStalled {
            route_id: c.running_route_id().unwrap(),
        });
        settle(&mut m, &mut t);
        assert_eq!(m.route_state(), &running("airpods"));
    }
    assert!(t - t0 < Duration::from_secs(10));
    assert_eq!(
        c.route_log(),
        [
            "build:airpods",
            "teardown:airpods",
            "build:airpods",
            "teardown:airpods",
            "build:airpods"
        ]
    );
    assert!(Arc::ptr_eq(&before, &m.engine()));
}

/// A stall from a route that was already replaced does nothing.
#[test]
fn a_stale_route_stall_is_ignored() {
    let (_d, s) = store();
    let (mut m, c, mut t) = start(&s);
    settle(&mut m, &mut t);
    c.emit(PlatformEvent::RouteStalled {
        route_id: uuid::Uuid::new_v4(),
    });
    settle(&mut m, &mut t);
    assert_eq!(c.route_log(), ["build:airpods"]);
    assert_eq!(m.route_state(), &running("airpods"));
}

#[test]
fn capture_denied_shows_permission_status_until_turned_off_and_on() {
    let (_d, s) = store();
    let (mut m, c, mut t) = start(&s);
    settle(&mut m, &mut t);
    c.emit(PlatformEvent::CaptureLooksDenied);
    settle(&mut m, &mut t);
    assert_eq!(
        m.route_state(),
        &RouteTarget::Failed(RouteFailure::Permission)
    );
    assert_eq!(
        status_text(&m),
        (
            "Allow AudioCurve to record system audio".to_string(),
            Tone::Warning
        )
    );
    assert_eq!(c.route_log(), ["build:airpods", "teardown:airpods"]);

    m.set_enabled(false, t);
    settle(&mut m, &mut t);
    assert_eq!(m.route_state(), &RouteTarget::Off);
    m.set_enabled(true, t);
    settle(&mut m, &mut t);
    assert_eq!(m.route_state(), &running("airpods"));
}

/// Swift `checkPermission`: once capture is seen working the permission is granted, and a later
/// silent stretch never latches the permission failure.
#[test]
fn capture_works_grants_the_permission_and_a_later_silence_is_not_denied() {
    let (_d, s) = store();
    let (mut m, c, mut t) = start(&s);
    settle(&mut m, &mut t);
    c.emit(PlatformEvent::CaptureWorks);
    settle(&mut m, &mut t);
    assert_eq!(m.route_state(), &running("airpods"));
    c.emit(PlatformEvent::CaptureLooksDenied);
    settle(&mut m, &mut t);
    assert_eq!(m.route_state(), &running("airpods"));
    assert_eq!(c.route_log(), ["build:airpods"]);
}

#[test]
fn other_live_hosts_are_reported() {
    let (_d, s) = store();
    let (mut m, _c, mut t) = start(&s);
    settle(&mut m, &mut t);
    assert!(m.other_hosts.is_empty());
    let mine = s.read_heartbeats();
    assert!(mine.iter().any(|b| b.pid == std::process::id()
        && b.host_bundle_id == "se.lindefors.audiocurve"
        && b.active));

    s.write_heartbeat(&InstanceHeartbeat {
        pid: u32::MAX - 1,
        host_bundle_id: "com.example.mixer".into(),
        active: true,
        time: SystemTime::now(),
    });
    settle(&mut m, &mut t);
    assert_eq!(m.other_hosts, ["com.example.mixer"]);
}

#[test]
fn quit_tears_down_first() {
    let (_d, s) = store();
    let p = s.save_new("P", &example_profile()).unwrap();
    let (mut m, c, mut t) = start(&s);
    settle(&mut m, &mut t);
    m.load_profile(p.id, t);
    m.editor.set_profile(edited(&p.profile), t);
    m.quit();
    assert_eq!(c.route_log(), ["build:airpods", "teardown:airpods"]);
    assert_eq!(m.route_state(), &RouteTarget::Off);
    assert_eq!(s.load_working_copy().unwrap().profile, edited(&p.profile));
    assert!(
        !s.read_heartbeats()
            .iter()
            .any(|b| b.pid == std::process::id())
    );

    m.set_enabled(true, t);
    settle(&mut m, &mut t);
    assert_eq!(c.route_log(), ["build:airpods", "teardown:airpods"]);
}

#[test]
fn hearing_test_status_wins() {
    let (_d, s) = store();
    let (mut m, _c, mut t) = start(&s);
    settle(&mut m, &mut t);
    m.testing = true;
    settle(&mut m, &mut t);
    assert_eq!(
        m.route_state(),
        &RouteTarget::Paused {
            device_uid: "airpods".into()
        }
    );
    assert_eq!(
        status_text(&m),
        ("Hearing test in progress".to_string(), Tone::Neutral)
    );
}

#[test]
fn output_switch_saves_a_never_saved_profile_as_unsaved() {
    let (_d, s) = store();
    let b = s.save_new("B", &HearingProfile::flat()).unwrap();
    let mut asg = OutputAssignments::default();
    asg.assign(&aids(), Some(b.id), None);
    s.save_assignments(&asg);

    let (mut m, c, mut t) = start(&s);
    m.editor.set_profile(example_profile(), t);
    let before = SystemTime::now();
    switch_to(&c, "aids");
    settle(&mut m, &mut t);
    let after = SystemTime::now();
    let unsaved: Vec<_> = s
        .list()
        .into_iter()
        .filter(|p| p.name.starts_with("Unsaved – "))
        .collect();
    assert_eq!(unsaved.len(), 1);
    // "Unsaved – <localized date> <HH:MM>".
    let named = |at| format!("Unsaved – {}", formatted_date_time(at, i18n::lang()));
    assert!(
        [named(before), named(after)].contains(&unsaved[0].name),
        "{}",
        unsaved[0].name
    );
    assert_eq!(unsaved[0].profile, example_profile());
    assert_eq!(m.editor.source_id, Some(b.id));
}

#[test]
fn levels_mode_and_toggles_are_kept_between_launches() {
    let (_d, s) = store();
    let (mut m, _c, mut t) = start(&s);
    settle(&mut m, &mut t);
    m.levels.mode = audiocurve_core::fitting::CompressionMode::Speech;
    m.levels.limiter = false;
    m.levels.compression = false;
    m.levels.strength = 80.0;
    m.levels.tone = 30.0;
    m.levels.mpo_dbfs = -7.0;
    m.levels.output_gain_db = -2.0;
    m.levels.input_gain_db = 4.0;
    let changed = m.levels;
    m.quit();

    let (m2, _c2, _t2) = start(&s);
    assert_eq!(m2.levels, changed);
}

#[test]
fn level_changes_are_saved_after_half_a_second() {
    let (_d, s) = store();
    let (mut m, _c, mut t) = start(&s);
    settle(&mut m, &mut t);
    m.levels.limiter = false;
    m.tick(t);
    assert!(m.next_deadline().unwrap() <= t + Duration::from_millis(500));
    m.tick(t + Duration::from_millis(400));
    assert!(audiocurve_app::model::levels::Levels::load(s.root()).limiter);
    m.tick(t + Duration::from_millis(500));
    assert!(!audiocurve_app::model::levels::Levels::load(s.root()).limiter);
}

#[test]
fn an_assigned_outputs_levels_win_over_the_kept_ones() {
    let (_d, s) = store();
    let p = s.save_new("P", &example_profile()).unwrap();
    let assigned = LevelSettings {
        strength: 20.0,
        input_gain_db: -1.0,
        output_gain_db: 1.0,
        tone: 5.0,
        mpo_dbfs: -10.0,
    };
    let mut a = OutputAssignments::default();
    a.assign(&airpods(), Some(p.id), Some(assigned));
    s.save_assignments(&a);
    let kept = audiocurve_app::model::levels::Levels {
        strength: 90.0,
        limiter: false,
        mode: audiocurve_core::fitting::CompressionMode::Music,
        ..Default::default()
    };
    kept.save(s.root());

    let (m, _c, _t) = start(&s);
    assert_eq!(m.levels.to_settings(), assigned);
    assert!(
        !m.levels.limiter,
        "what an assignment does not hold is kept"
    );
    assert_eq!(
        m.levels.mode,
        audiocurve_core::fitting::CompressionMode::Music
    );
}

#[test]
fn the_menu_bar_icon_follows_the_route_and_is_pushed_on_change() {
    use audiocurve_app::platform::TrayIconState;
    let (_d, s) = store();
    let (mut m, c, mut t) = start(&s);
    settle(&mut m, &mut t);
    assert_eq!(c.tray_icons().last(), Some(&TrayIconState::Active));

    m.set_enabled(false, t);
    settle(&mut m, &mut t);
    assert_eq!(c.tray_icons().last(), Some(&TrayIconState::Problem));
    let icons = c.tray_icons();
    assert!(
        icons.windows(2).all(|w| w[0] != w[1]),
        "pushed only when it changes: {icons:?}"
    );
}

#[test]
fn hiding_the_window_in_background_mode_turns_the_dock_icon_off() {
    let (_d, s) = store();
    let (mut m, c, _t) = start(&s);
    assert_eq!(c.dock_icon(), Some(true));
    m.set_window_open(false);
    assert!(!m.window_open);
    assert_eq!(c.dock_icon(), Some(false));
    m.set_window_open(true);
    assert_eq!(c.dock_icon(), Some(true));
}

#[test]
fn a_login_item_launch_starts_without_the_dock_icon() {
    let (_d, s) = store();
    let (p, c) = fake();
    let m = AppModel::new(
        p,
        s.clone(),
        Launch {
            from_login_item: true,
        },
        Instant::now(),
    );
    assert!(!m.window_open);
    assert_eq!(c.dock_icon(), Some(false));
}

#[test]
fn launch_at_login_is_asked_once_and_only_in_background_mode() {
    let (_d, s) = store();
    let (mut m, _c, _t) = start(&s);
    assert!(m.take_launch_at_login_question());
    assert!(!m.take_launch_at_login_question());
    drop(m);
    // Remembered between launches.
    let (mut m, _c, _t) = start(&s);
    assert!(!m.take_launch_at_login_question());

    let (_d2, s2) = store();
    Settings {
        background_processing: false,
        ..Settings::default()
    }
    .save(s2.root());
    let (mut m, _c, _t) = start(&s2);
    assert!(!m.take_launch_at_login_question());
    // Not marked as asked: turning background mode on later still asks once.
    m.set_background_processing(true);
    assert!(m.take_launch_at_login_question());
}
