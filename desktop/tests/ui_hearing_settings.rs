use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant, SystemTime};

use audiocurve_app::i18n::{self, Lang, formatted_date, formatted_date_time};
use audiocurve_app::model::app::{AppModel, Launch};
use audiocurve_app::model::hearing::Phase;
use audiocurve_app::platform::fake::{FakeControl, fake};
use audiocurve_app::ui::{hearing_test, settings, theme};
use audiocurve_core::hearing_test::TestCalibration;
use audiocurve_core::heartbeat::InstanceHeartbeat;
use audiocurve_core::store::ProfileStore;
use chrono::TimeZone;
use egui::Vec2;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

const SIZE: Vec2 = Vec2::new(900.0, 720.0);

/// The language is global; tests that read texts must not run while another switches it.
static LANG_LOCK: Mutex<()> = Mutex::new(());

struct Fixture {
    _lock: MutexGuard<'static, ()>,
    dir: tempfile::TempDir,
    store: ProfileStore,
    control: FakeControl,
    t: Instant,
}

fn model(lang: Lang) -> (Fixture, AppModel) {
    let lock = LANG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    i18n::set_lang(lang);
    let dir = tempfile::tempdir().unwrap();
    let store = ProfileStore::new(dir.path().to_path_buf());
    let (p, c) = fake();
    let mut t = Instant::now();
    let mut m = AppModel::new(
        p,
        store.clone(),
        Launch {
            from_login_item: false,
        },
        t,
    );
    for _ in 0..3 {
        t += Duration::from_millis(250);
        m.tick(t);
    }
    (
        Fixture {
            _lock: lock,
            dir,
            store,
            control: c,
            t,
        },
        m,
    )
}

/// 2026-11-16 at noon, local time.
fn fixed_date() -> SystemTime {
    chrono::Local
        .with_ymd_and_hms(2026, 11, 16, 12, 0, 0)
        .unwrap()
        .into()
}

fn phase(m: &mut AppModel) -> Phase {
    m.hearing().expect("session open").phase().clone()
}

/// Opens the test with a fixed random source: no catch trials, 2.5 s gaps.
fn open(m: &mut AppModel, t: Instant) {
    m.open_hearing_test_with_random(Box::new(|| 0.5), t);
}

/// Ticks deadline by deadline until the first tone plays.
fn until_tone(m: &mut AppModel, f: &mut Fixture) {
    for _ in 0..100 {
        if f.control.tone_log().iter().any(|e| e.starts_with("play:")) {
            return;
        }
        f.t = m.next_deadline().unwrap().max(f.t);
        m.tick(f.t);
    }
    panic!("no tone");
}

/// Runs the whole test with a listener that hears everything at or above 35 dB HL.
fn run_to_done(m: &mut AppModel, f: &mut Fixture) {
    m.hearing_start(f.t);
    let mut seen = f.control.tone_log().len();
    for _ in 0..100_000 {
        if matches!(phase(m), Phase::Done(_)) {
            return;
        }
        f.t = m.next_deadline().unwrap().max(f.t);
        m.tick(f.t);
        let log = f.control.tone_log();
        for e in &log[seen..] {
            let Some(rest) = e.strip_prefix("play:") else {
                continue;
            };
            let parts: Vec<&str> = rest.split(':').collect();
            let (Ok(hz), Ok(dbfs)) = (parts[1].parse::<f64>(), parts[2].parse::<f64>()) else {
                continue;
            };
            let cal = TestCalibration::nominal(m.hearing().unwrap().transducer());
            if cal.hl_from_dbfs(dbfs, hz) >= 35.0 - 1e-6 {
                m.hearing_respond(f.t);
            }
        }
        seen = log.len();
    }
    panic!("did not finish");
}

fn harness(m: AppModel) -> Harness<'static, AppModel> {
    let mut h = Harness::builder()
        .with_size(SIZE)
        .with_theme(egui::Theme::Light)
        .with_context_setup(theme::apply)
        .build_ui_state(
            |ui, m: &mut AppModel| hearing_test::show(ui, m, Instant::now()),
            m,
        );
    h.run();
    h
}

fn settings_harness(m: AppModel) -> Harness<'static, AppModel> {
    let mut h = Harness::builder()
        .with_size(Vec2::new(420.0, 300.0))
        .with_context_setup(theme::apply)
        .build_ui_state(|ui, m: &mut AppModel| settings::contents(ui, m), m);
    h.run();
    h
}

#[test]
fn each_hearing_phase_renders() {
    let (mut f, mut m) = model(Lang::En);
    open(&mut m, f.t);
    let mut h = harness(m);
    h.get_by_label("Before you start");
    h.get_by_label("Output: AirPods Pro");
    h.get_by_label("In-ear");
    h.render().expect("intro");

    let m = h.state_mut();
    m.hearing_start(f.t);
    until_tone(m, &mut f);
    h.run();
    h.get_by_label("I hear it");
    h.get_by_label("Press Space or click when you hear a tone.");
    h.get_by_label("1 / 24");
    h.render().expect("running");

    h.get_by_label("Pause").click();
    h.run();
    assert!(matches!(phase(h.state_mut()), Phase::Paused(_)));
    h.get_by_label("Paused");
    h.get_by_label("Resume");
    h.render().expect("paused");

    let m = h.state_mut();
    m.hearing_cancel(f.t);
    open(m, f.t);
    run_to_done(m, &mut f);
    h.run();
    h.get_by_label("Done");
    h.get_by_label("Relative to AirPods Pro");
    h.get_by_label("Audiogram");
    h.get_by_label("Use on AirPods Pro");
    h.render().expect("done");

    // A file where the profiles folder should be makes the save fail.
    let profiles = f.dir.path().join("profiles");
    let _ = std::fs::remove_dir_all(&profiles);
    std::fs::write(&profiles, b"").unwrap();
    h.get_by_label("Save").click();
    h.run();
    assert_eq!(phase(h.state_mut()), Phase::SaveFailed);
    h.get_by_label("The profile could not be saved.");
    h.get_by_label("Retry");
    h.render().expect("save failed");
}

#[test]
fn i_hear_it_button_responds() {
    let (mut f, mut m) = model(Lang::En);
    open(&mut m, f.t);
    m.hearing_start(f.t);
    until_tone(&mut m, &mut f);
    let Phase::Running(before) = phase(&mut m) else {
        panic!("not running");
    };
    let mut h = harness(m);
    h.get_by_label("I hear it").click();
    h.run();
    let Phase::Running(after) = phase(h.state_mut()) else {
        panic!("not running");
    };
    // Heard: the staircase goes down 10 dB for the next presentation.
    assert!(after.level_hl < before.level_hl, "{before:?} -> {after:?}");
}

#[test]
fn space_responds_once_per_press() {
    let (mut f, mut m) = model(Lang::En);
    open(&mut m, f.t);
    m.hearing_start(f.t);
    until_tone(&mut m, &mut f);
    let Phase::Running(before) = phase(&mut m) else {
        panic!("not running");
    };
    let mut h = harness(m);
    h.key_press(egui::Key::Space);
    h.run();
    let Phase::Running(after) = phase(h.state_mut()) else {
        panic!("not running");
    };
    assert!(after.level_hl < before.level_hl, "{before:?} -> {after:?}");
}

#[test]
fn done_screen_saves_with_default_name() {
    let (mut f, mut m) = model(Lang::En);
    open(&mut m, f.t);
    run_to_done(&mut m, &mut f);
    m.hearing().unwrap().set_test_date(fixed_date());
    let mut h = harness(m);
    h.get_by_label("Save").click();
    h.run();
    assert!(h.state_mut().hearing().is_none());
    let names: Vec<String> = f.store.list().into_iter().map(|s| s.name).collect();
    assert_eq!(names, ["AirPods Pro – 16 Nov 2026"]);
    // "Use on AirPods Pro" is on by default.
    assert!(
        h.state()
            .assignments
            .assignment("airpods")
            .and_then(|a| a.profile_id)
            .is_some()
    );
}

#[test]
fn blank_name_saves_under_the_localized_default_name() {
    let (mut f, mut m) = model(Lang::Sv);
    open(&mut m, f.t);
    run_to_done(&mut m, &mut f);
    m.hearing().unwrap().set_test_date(fixed_date());
    m.hearing_save("  ", false);
    let names: Vec<String> = f.store.list().into_iter().map(|s| s.name).collect();
    assert_eq!(names, ["AirPods Pro – 16 nov. 2026"]);
}

#[test]
fn another_host_is_named_by_its_app_name() {
    let (mut f, mut m) = model(Lang::En);
    f.store.write_heartbeat(&InstanceHeartbeat {
        pid: u32::MAX - 1,
        host_bundle_id: "com.example.mixer".into(),
        active: true,
        time: SystemTime::now(),
    });
    for _ in 0..12 {
        f.t += Duration::from_millis(250);
        m.tick(f.t);
    }
    open(&mut m, f.t);
    assert_eq!(m.hearing_blocking_host_name().as_deref(), Some("mixer"));
    let h = harness(m);
    h.get_by_label("AudioCurve is running in mixer. Quit it to start the test.");
}

#[test]
fn formatted_date_in_every_language() {
    let d = fixed_date();
    let expected = [
        (Lang::En, "16 Nov 2026"),
        (Lang::Sv, "16 nov. 2026"),
        (Lang::Da, "16. nov. 2026"),
        (Lang::Nb, "16. nov. 2026"),
        (Lang::Fi, "16.11.2026"),
        (Lang::De, "16. Nov. 2026"),
        (Lang::Fr, "16 nov. 2026"),
        (Lang::Es, "16 nov 2026"),
    ];
    for (lang, s) in expected {
        assert_eq!(formatted_date(d, lang), s, "{lang:?}");
    }
}

#[test]
fn formatted_date_time_appends_hours_and_minutes() {
    let d: SystemTime = chrono::Local
        .with_ymd_and_hms(2026, 3, 5, 9, 7, 0)
        .unwrap()
        .into();
    assert_eq!(formatted_date_time(d, Lang::En), "5 Mar 2026 09:07");
    assert_eq!(formatted_date_time(d, Lang::Fi), "5.3.2026 09:07");
}

#[test]
fn settings_hides_launch_at_login_in_foreground_mode() {
    let (f, mut m) = model(Lang::En);
    m.set_background_processing(false);
    let h = settings_harness(m);
    h.get_by_label("Process all sound in the background");
    h.get_by_label("Show in menu bar");
    assert!(h.query_by_label("Launch at login").is_none());
    h.get_by_label(
        "AudioCurve processes sound only while its window is open and Bypass is off. Closing the window quits AudioCurve.",
    );
    drop(h);
    drop(f);

    let (_f, mut m) = model(Lang::En);
    m.set_background_processing(true);
    let h = settings_harness(m);
    h.get_by_label("Launch at login");
    h.get_by_label("AudioCurve keeps processing what your Mac plays when its window is closed.");
}

#[test]
fn settings_toggles_change_the_settings() {
    let (_f, m) = model(Lang::En);
    assert!(m.settings.show_in_menu_bar);
    let mut h = settings_harness(m);
    h.get_by_label("Show in menu bar").click();
    h.run();
    assert!(!h.state().settings.show_in_menu_bar);
    h.get_by_label("Open AudioCurve from its app icon to get back to the window.");
    h.get_by_label("Launch at login").click();
    h.run();
    assert!(h.state().launch_at_login());
    h.get_by_label("Process all sound in the background")
        .click();
    h.run();
    assert!(!h.state().settings.background_processing);
    assert!(!h.state().launch_at_login());
}
