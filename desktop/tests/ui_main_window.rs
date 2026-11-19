mod support;

use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

use audiocurve_app::i18n::{self, Lang, tr};
use audiocurve_app::model::app::{AppModel, Launch};
use audiocurve_app::model::editor::Pane;
use audiocurve_app::model::status::status_text;
use audiocurve_app::platform::PlatformEvent;
use audiocurve_app::platform::fake::{FakeControl, fake};
use audiocurve_app::ui::{main_window, theme};
use audiocurve_core::route::{RouteFailure, RouteTarget};
use audiocurve_core::store::ProfileStore;
use egui::{Modifiers, Vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use support::example_profile;

const SIZE: Vec2 = Vec2::new(900.0, 720.0);

/// The language is global; tests that read texts must not run while another switches it.
static LANG_LOCK: Mutex<()> = Mutex::new(());

struct Fixture {
    _lock: MutexGuard<'static, ()>,
    _dir: tempfile::TempDir,
    control: FakeControl,
    t: Instant,
}

/// Ticks at each deadline for up to 5 s.
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

fn model(lang: Lang) -> (Fixture, AppModel) {
    let lock = LANG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    i18n::set_lang(lang);
    let dir = tempfile::tempdir().unwrap();
    let store = ProfileStore::new(dir.path().to_path_buf());
    let (p, c) = fake();
    let mut t = Instant::now();
    let mut m = AppModel::new(
        p,
        store,
        Launch {
            from_login_item: false,
        },
        t,
    );
    m.editor.set_profile(example_profile(), t);
    settle(&mut m, &mut t);
    (
        Fixture {
            _lock: lock,
            _dir: dir,
            control: c,
            t,
        },
        m,
    )
}

fn harness(m: AppModel, theme: egui::Theme) -> Harness<'static, AppModel> {
    let mut h = Harness::builder()
        .with_size(SIZE)
        .with_theme(theme)
        .with_context_setup(theme::apply)
        .build_ui_state(
            |ui, m: &mut AppModel| main_window::show(ui, m, Instant::now()),
            m,
        );
    h.run();
    h
}

#[test]
fn main_window_renders_light_and_dark() {
    for t in [egui::Theme::Light, egui::Theme::Dark] {
        let (_f, m) = model(Lang::En);
        let mut h = harness(m, t);
        h.render().expect("render");
        drop(h);
    }
}

#[test]
fn limiter_off_asks_for_confirmation() {
    let (_f, m) = model(Lang::En);
    let mut h = harness(m, egui::Theme::Light);
    assert!(h.state().levels.limiter);
    h.get_by_label("Limiter").click();
    h.run();
    // Still on until confirmed.
    assert!(h.state().levels.limiter);
    h.get_by_label("Turn Off").click();
    h.run();
    assert!(!h.state().levels.limiter);
    assert!(h.query_by_label("Turn Off").is_none());
}

#[test]
fn limiter_off_cancel_keeps_it_on() {
    let (_f, m) = model(Lang::En);
    let mut h = harness(m, egui::Theme::Light);
    h.get_by_label("Limiter").click();
    h.run();
    h.get_by_label("Cancel").click();
    h.run();
    assert!(h.state().levels.limiter);
    assert!(h.query_by_label("Turn Off").is_none());
}

#[test]
fn save_as_prompt_saves_a_version() {
    let (_f, m) = model(Lang::En);
    assert!(m.editor.saved.is_empty());
    let mut h = harness(m, egui::Theme::Light);
    h.get_by_label("Unsaved profile •").click();
    h.run();
    h.get_by_label("Save As…").click();
    h.run();
    h.get_by_label("Name").focus();
    h.run();
    h.key_press_modifiers(Modifiers::COMMAND, egui::Key::A);
    h.get_by_label("Name").type_text("Kitchen");
    h.run();
    h.get_by_label("Save").click();
    h.run();
    let e = &h.state().editor;
    assert!(e.saved.iter().any(|s| s.name == "Kitchen"));
    assert_eq!(e.current_name(), Some("Kitchen"));
    assert!(h.query_by_label("Name").is_none());
}

#[test]
fn bypass_checkbox_sets_bypass() {
    let (_f, m) = model(Lang::En);
    let before = m.bypass;
    let mut h = harness(m, egui::Theme::Light);
    h.get_by_label("Bypass (A/B)").click();
    h.run();
    assert_eq!(h.state().bypass, !before);
}

#[test]
fn status_line_shows_the_status_text() {
    let (_f, m) = model(Lang::En);
    let (text, _) = status_text(&m);
    assert!(text.starts_with("Active – "), "{text}");
    let h = harness(m, egui::Theme::Light);
    h.get_by_label(&text);
}

#[test]
fn latched_engine_failure_offers_try_again() {
    let (mut f, mut m) = model(Lang::En);
    for _ in 0..2 {
        f.control.emit(PlatformEvent::RenderFailed {
            route_id: f.control.running_route_id().unwrap(),
        });
        settle(&mut m, &mut f.t);
    }
    assert_eq!(m.route_state(), &RouteTarget::Failed(RouteFailure::Plugin));
    let mut h = harness(m, egui::Theme::Light);
    h.get_by_label("The sound processing stopped");
    h.get_by_label("Try Again").click();
    h.run();
    assert_ne!(
        h.state().route_state(),
        &RouteTarget::Failed(RouteFailure::Plugin)
    );
}

#[test]
fn pane_control_switches_the_plot() {
    let (_f, m) = model(Lang::En);
    let mut h = harness(m, egui::Theme::Light);
    h.get_by_label("Hearing threshold, dB HL");
    h.get_by_label("Spectrum").click();
    h.run();
    assert_eq!(h.state().editor.pane, Pane::Spectrum);
    h.get_by_label("Spectrum, dBFS");
    h.get_by_label("Gain").click();
    h.run();
    assert_eq!(h.state().editor.pane, Pane::Gain);
    h.get_by_label("Gain per band, dB");
}

#[test]
fn hearing_test_button_opens_the_session() {
    let (_f, m) = model(Lang::En);
    let mut h = harness(m, egui::Theme::Light);
    h.get_by_label("Hearing test…").click();
    h.run();
    assert!(h.state_mut().hearing().is_some());
    assert!(h.query_by_label("Limiter").is_none());
}

#[test]
fn swedish_ui_renders() {
    let (_f, m) = model(Lang::Sv);
    let mut h = harness(m, egui::Theme::Light);
    h.get_by_label("Aktiv på: alla utgångar");
    h.get_by_label("Limiter");
    h.get_by_label("Hörseltest…");
    h.render().expect("render");
}

#[test]
fn active_on_popup_unticks_an_output() {
    let (_f, m) = model(Lang::En);
    assert!(m.active_outputs.is_all());
    let mut h = harness(m, egui::Theme::Light);
    h.get_by_label("Active on: all outputs").click();
    h.run();
    h.get_by_label("Hearing Aids").click();
    h.run();
    let a = &h.state().active_outputs;
    assert!(!a.is_all());
    assert!(!a.is_checked("aids"));
    assert!(a.is_checked("airpods"));
}

#[test]
fn use_on_output_in_profile_menu_assigns_the_version() {
    let (_f, mut m) = model(Lang::En);
    m.editor.save_as("Kitchen");
    let uid = m.current_output.clone().unwrap().uid;
    let label = format!("Use on {}", m.current_output.as_ref().unwrap().name);
    let mut h = harness(m, egui::Theme::Light);
    // Not in the Active on popup.
    h.get_by_label("Active on: all outputs").click();
    h.run();
    assert!(h.query_by_label(&label).is_none());
    h.key_press(egui::Key::Escape);
    h.run();
    h.get_by_label("Kitchen").click();
    h.run();
    h.get_by_label(&label).click();
    h.run();
    let s = h.state();
    assert!(s.editor.source_id.is_some());
    assert_eq!(
        s.assignments.assignment(&uid).and_then(|a| a.profile_id),
        s.editor.source_id
    );
}

fn harness_sized(m: AppModel, size: Vec2) -> Harness<'static, AppModel> {
    let mut h = Harness::builder()
        .with_size(size)
        .with_context_setup(theme::apply)
        .build_ui_state(
            |ui, m: &mut AppModel| main_window::show(ui, m, Instant::now()),
            m,
        );
    h.run();
    h
}

/// Swift's minimum window size (`HostView`); `main` uses it too.
const MIN_SIZE: Vec2 = Vec2::new(720.0, 700.0);

#[test]
fn minimum_size_main_window_keeps_controls_visible_and_beside_the_plot() {
    for (lang, pane) in [
        Lang::En,
        Lang::Sv,
        Lang::Da,
        Lang::Nb,
        Lang::Fi,
        Lang::De,
        Lang::Fr,
        Lang::Es,
    ]
    .iter()
    .flat_map(|l| [(*l, Pane::Audiogram), (*l, Pane::Gain)])
    {
        let (f, mut m) = model(lang);
        // The tallest panel and the widest header: limiter off, a long version name.
        m.levels.limiter = false;
        let p = m.editor.profile.clone().unwrap();
        m.editor.save_as("Living room with the big speakers");
        m.editor.set_profile(p, f.t);
        assert!(!m.editor.is_dirty());
        m.editor.pane = pane;
        let mut h = harness_sized(m, MIN_SIZE);
        let view = egui::Rect::from_min_size(egui::Pos2::ZERO, MIN_SIZE);
        let (limiter, auto, meter) = (tr("Limiter"), tr("Auto level"), tr("Safety, reduction"));
        // Limiter and Auto level are in view as is; the meters at least by scrolling the panel.
        let meter_bottom = h.get_by_label(&meter).rect().bottom();
        for label in [&limiter, &auto] {
            let r = h.get_by_label(label).rect();
            assert!(
                view.contains_rect(r) && r.bottom() < meter_bottom,
                "{lang:?} {label}: {r:?} outside {view:?}"
            );
        }
        h.get_by_label(&meter).scroll_to_me();
        h.run();
        let r = h.get_by_label(&meter).rect();
        assert!(
            view.contains_rect(r),
            "{lang:?} {meter}: {r:?} outside {view:?}"
        );
        // The panel's left edge is the Auto level button's, less the frame's inner margin.
        let panel_left = h.get_by_label(&auto).rect().left() - 14.0;
        let plot_label = if pane == Pane::Gain {
            tr("Gain per band")
        } else {
            tr("Audiogram")
        };
        let plot = h
            .get_by_role_and_label(egui::accesskit::Role::Canvas, &plot_label)
            .rect();
        assert!(
            plot.right() <= panel_left,
            "{lang:?}: plot {plot:?} runs under the panel at {panel_left}"
        );
        let caption = if pane == Pane::Gain {
            tr("Gain per band, dB")
        } else {
            tr("Hearing threshold, dB HL")
        };
        for label in [tr("Living room with the big speakers"), caption] {
            if let Some(n) = h.query_by_label(&label) {
                let r = n.rect();
                assert!(
                    r.right() <= panel_left,
                    "{lang:?} {label}: {r:?} runs under the panel at {panel_left}"
                );
            }
        }
    }
}

#[test]
fn minimum_window_size_is_swifts() {
    assert_eq!(main_window::MIN_WINDOW_SIZE, [720.0, 700.0]);
}

#[test]
fn controls_scroll_when_the_window_is_short() {
    let (_f, mut m) = model(Lang::De);
    m.levels.limiter = false;
    let size = Vec2::new(720.0, 460.0);
    let mut h = harness_sized(m, size);
    let view = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
    for label in [tr("Limiter"), tr("Auto level"), tr("Safety, reduction")] {
        h.get_by_label(&label).scroll_to_me();
        h.run();
        let r = h.get_by_label(&label).rect();
        assert!(view.contains_rect(r), "{label}: {r:?} outside {view:?}");
    }
}
