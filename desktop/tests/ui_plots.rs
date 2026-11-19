mod support;

use std::time::{Duration, Instant};

use audiocurve_app::i18n::{self, Lang};
use audiocurve_app::model::app::{AppModel, Launch};
use audiocurve_app::model::editor::{Editor, Pane};
use audiocurve_app::platform::fake::{FakeControl, fake};
use audiocurve_app::ui::geometry::PlotGeometry;
use audiocurve_app::ui::spectrum::SpectrumEars;
use audiocurve_app::ui::{audiogram, gain, spectrum, theme, widgets};
use audiocurve_core::editing::PointSelection;
use audiocurve_core::profile::Ear;
use audiocurve_core::store::ProfileStore;
use egui::{Event, Modifiers, PointerButton, Pos2, Vec2, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use support::example_profile;

const SIZE: Vec2 = Vec2::new(640.0, 420.0);

fn editor() -> (tempfile::TempDir, Editor) {
    i18n::set_lang(Lang::En);
    let dir = tempfile::tempdir().unwrap();
    let mut e = Editor::new(ProfileStore::new(dir.path().to_path_buf()));
    e.set_profile(example_profile(), Instant::now());
    (dir, e)
}

fn model() -> (tempfile::TempDir, AppModel, FakeControl) {
    i18n::set_lang(Lang::En);
    let dir = tempfile::tempdir().unwrap();
    let store = ProfileStore::new(dir.path().to_path_buf());
    let (p, c) = fake();
    let now = Instant::now();
    let mut m = AppModel::new(
        p,
        store,
        Launch {
            from_login_item: false,
        },
        now,
    );
    m.editor.set_profile(example_profile(), now);
    // Ticks at each deadline for up to 5 s, so the route gets built and runs.
    let end = now + Duration::from_secs(5);
    let mut t = now;
    m.tick(t);
    while let Some(d) = m.next_deadline().filter(|d| *d <= end) {
        t = d.max(t);
        m.tick(t);
    }
    assert!(c.running_route_id().is_some());
    (dir, m, c)
}

fn audiogram_harness(e: Editor, theme: egui::Theme) -> Harness<'static, Editor> {
    Harness::builder()
        .with_size(SIZE)
        .with_theme(theme)
        .with_context_setup(theme::apply)
        .build_ui_state(
            |ui, e: &mut Editor| audiogram::show(ui, e, Instant::now()),
            e,
        )
}

fn press(h: &mut Harness<'_, Editor>, pos: Pos2) {
    h.event(Event::PointerMoved(pos));
    h.event(Event::PointerButton {
        pos,
        button: PointerButton::Primary,
        pressed: true,
        modifiers: Modifiers::NONE,
    });
    h.run();
}

fn release(h: &mut Harness<'_, Editor>, pos: Pos2) {
    h.event(Event::PointerButton {
        pos,
        button: PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    h.run();
}

/// The screen position of a value in the audiogram plot, and the plot's geometry.
fn audiogram_pos(h: &Harness<'_, Editor>, f: f64, db: f64) -> Pos2 {
    let rect = h.get_by_label("Audiogram").rect();
    let g = PlotGeometry::new(rect.width() as f64, rect.height() as f64);
    rect.min + vec2(g.x(f) as f32, g.y(db) as f32)
}

fn threshold(e: &Editor, ear: Ear, f: f64) -> f64 {
    e.profile
        .as_ref()
        .unwrap()
        .audiogram(ear)
        .points
        .iter()
        .find(|q| q.frequency_hz == f)
        .unwrap()
        .threshold_db_hl
}

#[test]
fn audiogram_renders_light_and_dark() {
    for t in [egui::Theme::Light, egui::Theme::Dark] {
        let (_d, e) = editor();
        let mut h = audiogram_harness(e, t);
        h.run();
        h.render().expect("render");
    }
}

#[test]
fn clicking_a_point_selects_it() {
    let (_d, e) = editor();
    let mut h = audiogram_harness(e, egui::Theme::Light);
    h.run();
    let pos = audiogram_pos(&h, 1000.0, 55.0);
    press(&mut h, pos);
    release(&mut h, pos);
    assert_eq!(
        h.state().selection,
        Some(PointSelection {
            ear: Ear::Left,
            frequency_hz: 1000.0
        })
    );
    assert!(h.get_by_label("Audiogram").is_focused());
}

#[test]
fn dragging_a_point_changes_its_threshold() {
    let (_d, e) = editor();
    let mut h = audiogram_harness(e, egui::Theme::Light);
    h.run();
    let start = audiogram_pos(&h, 1000.0, 55.0);
    let end = audiogram_pos(&h, 1000.0, 75.0);
    press(&mut h, start);
    for i in 1..=4 {
        let p = start + (end - start) * (i as f32 / 4.0);
        h.event(Event::PointerMoved(p));
        h.run();
    }
    release(&mut h, end);
    assert_eq!(threshold(h.state(), Ear::Left, 1000.0), 75.0);
}

#[test]
fn arrow_keys_nudge_the_selected_point_and_cmd_is_ignored() {
    let (_d, e) = editor();
    let mut h = audiogram_harness(e, egui::Theme::Light);
    h.run();
    let pos = audiogram_pos(&h, 1000.0, 55.0);
    press(&mut h, pos);
    release(&mut h, pos);
    h.key_press(egui::Key::ArrowUp);
    h.run();
    assert_eq!(threshold(h.state(), Ear::Left, 1000.0), 50.0);
    h.key_press_modifiers(Modifiers::ALT, egui::Key::ArrowUp);
    h.run();
    assert_eq!(threshold(h.state(), Ear::Left, 1000.0), 49.0);
    h.key_press_modifiers(Modifiers::COMMAND, egui::Key::ArrowUp);
    h.key_press_modifiers(Modifiers::CTRL, egui::Key::ArrowUp);
    h.run();
    assert_eq!(threshold(h.state(), Ear::Left, 1000.0), 49.0);
    // Tab stays in the plot and switches the ear.
    h.key_press(egui::Key::Tab);
    h.run();
    assert_eq!(h.state().selection.map(|s| s.ear), Some(Ear::Right));
    assert!(h.get_by_label("Audiogram").is_focused());
}

#[test]
fn gain_and_spectrum_render_after_pumping_noise() {
    let (_d, mut m, c) = model();
    for _ in 0..20 {
        c.pump(2400);
    }
    m.tick(Instant::now());
    m.editor.pane = Pane::Gain;
    struct S {
        m: AppModel,
        ears: SpectrumEars,
    }
    let state = S {
        m,
        ears: SpectrumEars::Both,
    };
    let mut h = Harness::builder()
        .with_size(vec2(640.0, 840.0))
        .with_context_setup(theme::apply)
        .build_ui_state(
            |ui, s: &mut S| {
                ui.allocate_ui(vec2(600.0, 400.0), |ui| {
                    gain::show(ui, &mut s.m, Instant::now())
                });
                ui.allocate_ui(vec2(600.0, 400.0), |ui| {
                    spectrum::show(ui, &s.m, &mut s.ears)
                });
            },
            state,
        );
    h.run();
    h.render().expect("render");
    let _ = h.get_by_label("Gain per band");
    let value = h.get_by_label("Spectrum").value().unwrap_or_default();
    assert!(value.starts_with("Left: 125 Hz"), "{value}");
    assert!(value.contains("Right:"), "{value}");
    // The pumped noise shows: no level at the spectrum floor.
    assert!(!value.contains("−140 dBFS"), "{value}");

    h.get_by_label("L").click();
    h.run();
    assert_eq!(h.state().ears, SpectrumEars::Left);
    let value = h.get_by_label("Spectrum").value().unwrap_or_default();
    assert!(!value.contains("Right:"), "{value}");
}

#[test]
fn segmented_control_selects_on_click() {
    #[derive(Clone, Copy, PartialEq, Debug)]
    enum Mode {
        Speech,
        Music,
        Auto,
    }
    let mut h = Harness::builder()
        .with_size(vec2(400.0, 100.0))
        .with_context_setup(theme::apply)
        .build_ui_state(
            |ui, mode: &mut Mode| {
                widgets::segmented(
                    ui,
                    mode,
                    &[
                        (Mode::Speech, "Speech"),
                        (Mode::Music, "Music"),
                        (Mode::Auto, "Auto"),
                    ],
                );
            },
            Mode::Auto,
        );
    h.run();
    h.get_by_label("Music").click();
    h.run();
    assert_eq!(*h.state(), Mode::Music);
    h.render().expect("render");
}

#[test]
fn labeled_slider_and_meters_render() {
    let mut h = Harness::builder()
        .with_size(vec2(260.0, 200.0))
        .with_context_setup(theme::apply)
        .build_ui_state(
            |ui, v: &mut f64| {
                widgets::labeled_slider(ui, "Strength", v, 0.0..=100.0, &format!("{v:.0} %"));
                widgets::meter_pair(ui, "In", -20.0, -30.0, Some([-10.0, -0.5]));
                widgets::meter_pair(ui, "Safety", f32::NEG_INFINITY, f32::NAN, None);
            },
            50.0,
        );
    h.run();
    let _ = h.get_by_role_and_label(egui::Role::Slider, "Strength");
    h.render().expect("render");
}

#[test]
fn tiny_and_zero_sized_plots_do_not_panic() {
    assert!(PlotGeometry::new(0.0, 0.0).y(50.0).is_finite());
    for size in [vec2(600.0, 420.0), vec2(600.0, 0.0), vec2(0.0, 0.0)] {
        let (_d, mut m, c) = model();
        c.pump(4800);
        m.tick(Instant::now());
        m.editor.selection = Some(PointSelection {
            ear: Ear::Left,
            frequency_hz: 1000.0,
        });
        let mut ears = SpectrumEars::Right;
        let mut h = Harness::builder()
            .with_size(vec2(640.0, 480.0))
            .with_context_setup(theme::apply)
            .build_ui_state(
                move |ui, m: &mut AppModel| {
                    for pane in [Pane::Audiogram, Pane::Gain, Pane::Spectrum] {
                        m.editor.pane = pane;
                        ui.allocate_ui(size, |ui| match pane {
                            Pane::Audiogram => audiogram::show(ui, &mut m.editor, Instant::now()),
                            Pane::Gain => gain::show(ui, m, Instant::now()),
                            Pane::Spectrum => spectrum::show(ui, m, &mut ears),
                        });
                    }
                },
                m,
            );
        h.run();
        h.render().expect("render");
    }
}
