//! The hearing test in the main window (Swift `HearingTestView`): the intro, the running test,
//! the paused test and the result with its save form.

use std::f32::consts::TAU;
use std::time::{Duration, Instant};

use audiocurve_core::hearing_test::{
    HearingTest, HearingTestResult, Presentation, TestWarning, Transducer,
};
use audiocurve_core::profile::Ear;
use egui::{
    Align, Align2, Button, Color32, Event, FontId, Id, Key, Label, Layout, ProgressBar, Rect,
    Response, RichText, Role, Sense, Shape, Stroke, TextEdit, Ui, UiBuilder, WidgetInfo, pos2,
    vec2,
};

use crate::i18n::{self, Lang, tr, trf};
use crate::model::app::AppModel;
use crate::model::hearing::{PauseReason, Phase, StopReason};
use crate::ui::widgets::{secondary, segmented};
use crate::ui::{audiogram, theme};

const SECTION_SPACING: f32 = 14.0;
const EAR_SIZE: f32 = 170.0;
const EAR_SPACING: f32 = 96.0;
const PULSE_PERIOD_S: f64 = 1.2;

/// What the screens show of the session, read once per frame.
struct View {
    phase: Phase,
    /// The output the test runs on, else the current output.
    output_name: Option<String>,
    /// The output the test ran on.
    device_name: String,
    stop_reason: Option<StopReason>,
    remind_volume_down: bool,
    blocking_host: Option<String>,
    muted: bool,
    others_playing: bool,
    low_sample_rate: bool,
    volume_settable: bool,
    can_start: bool,
    can_resume: bool,
    transducer: Transducer,
    default_name: String,
}

impl View {
    fn read(m: &mut AppModel) -> Option<View> {
        let current = m.current_output.as_ref().map(|d| d.name.clone());
        let blocking_host = m.hearing_blocking_host_name();
        let h = m.hearing()?;
        let device_name = h.device().map(|d| d.name.clone());
        Some(View {
            phase: h.phase().clone(),
            output_name: device_name.clone().or(current),
            device_name: device_name.unwrap_or_default(),
            stop_reason: h.stop_reason(),
            remind_volume_down: h.remind_volume_down(),
            blocking_host,
            muted: h.muted(),
            others_playing: h.others_playing(),
            low_sample_rate: h.low_sample_rate(),
            volume_settable: h.volume_settable(),
            can_start: h.can_start(),
            can_resume: h.can_resume(),
            transducer: h.transducer(),
            default_name: h.localized_default_name(),
        })
    }
}

/// The name and "Use on <output>" of the save form, kept while the result is shown.
#[derive(Clone)]
struct SaveForm {
    name: String,
    assign: bool,
}

pub fn show(ui: &mut Ui, m: &mut AppModel, now: Instant) {
    let Some(v) = View::read(m) else {
        return;
    };
    ui.spacing_mut().item_spacing.y = 10.0;
    ui.horizontal(|ui| {
        ui.label(RichText::new(tr("Hearing test")).size(20.0).strong());
        if let Some(name) = &v.output_name {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let c = secondary(ui.visuals());
                ui.add(Label::new(RichText::new(trf("Output: %@", &[name])).color(c)).truncate());
            });
        }
    });
    ui.add_space(6.0);
    let form_id = ui.scope_id().with("hearing_save_form");
    match &v.phase {
        Phase::Intro => intro(ui, m, &v, now),
        Phase::Running(p) => running(ui, m, p, now),
        Phase::Paused(reason) => paused(ui, m, &v, *reason, now),
        Phase::Done(r) => result(ui, m, &v, r, form_id, now),
        Phase::SaveFailed => {
            let r = m.hearing().and_then(|h| h.result().cloned());
            if let Some(r) = r {
                result(ui, m, &v, &r, form_id, now);
            }
        }
    }
    if !matches!(v.phase, Phase::Done(_) | Phase::SaveFailed) {
        ui.data_mut(|d| d.remove::<SaveForm>(form_id));
    }
}

// MARK: Intro

fn intro(ui: &mut Ui, m: &mut AppModel, v: &View, now: Instant) {
    ui.spacing_mut().item_spacing.y = SECTION_SPACING;
    headline(ui, &tr("Before you start"));
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = 6.0;
        bullet(
            ui,
            &tr("Sit somewhere quiet. Turn on noise cancellation if your headphones have it."),
        );
        bullet(ui, &tr("Stop other sound on the Mac."));
        bullet(
            ui,
            &tr("Turn on Do Not Disturb so alerts don't play during the test."),
        );
        bullet(ui, &tr("Turn off any system-wide EQ or sound enhancer."));
        bullet(
            ui,
            &tr(if v.volume_settable {
                "The test sets the headphone volume to max and controls the level itself. The volume is restored afterwards."
            } else {
                "The test controls the level itself. Turn the volume back down when it ends."
            }),
        );
    });

    ui.horizontal(|ui| {
        ui.label(tr("Headphones"));
        let mut t = v.transducer;
        let (a, b) = (tr("In-ear"), tr("Over-ear"));
        let r = segmented(
            ui,
            &mut t,
            &[
                (Transducer::InEar, a.as_str()),
                (Transducer::OverEar, b.as_str()),
            ],
        );
        if r.changed()
            && let Some(h) = m.hearing()
        {
            h.set_transducer_choice(Some(t));
        }
    });

    let mut warnings = Vec::new();
    if let Some(reason) = v.stop_reason {
        warnings.push(stop_text(reason));
    }
    if v.remind_volume_down {
        warnings.push(tr("Turn the volume back down now."));
    }
    if let Some(host) = &v.blocking_host {
        warnings.push(trf(
            "AudioCurve is running in %@. Quit it to start the test.",
            &[host],
        ));
    }
    if v.muted {
        warnings.push(tr("The output is muted. Unmute it to start the test."));
    }
    if v.others_playing {
        warnings.push(tr(
            "Another app is playing sound. Stop it before you start.",
        ));
    }
    if v.low_sample_rate {
        warnings.push(low_sample_rate_text());
    }
    if !v.volume_settable {
        warnings.push(tr("AudioCurve cannot set the volume of this output, so the level may be off. Set the volume to max yourself before you start."));
    }
    warning_list(ui, &warnings);

    ui.horizontal(|ui| {
        let start = prominent(ui, &tr("Start test"), v.can_start).clicked();
        let cancel = ui.button(tr("Cancel")).clicked();
        if cancel {
            m.hearing_cancel(now);
        } else if start || (v.can_start && default_action(ui)) {
            m.hearing_start(now);
        }
    });
}

fn stop_text(reason: StopReason) -> String {
    tr(match reason {
        StopReason::OutputChanged => "The test stopped because the output changed.",
        StopReason::OtherHost => "The test stopped because AudioCurve started in another app.",
        StopReason::WindowClosed => "The test stopped because the window was closed.",
        StopReason::GeneratorStopped => "The test stopped because the output stopped playing.",
        StopReason::GeneratorFailed => "The test tones could not be played on this output.",
        StopReason::VolumeNotSet => {
            "The volume could not be set to max, so the test did not start."
        }
        StopReason::LowSampleRate => {
            "The test stopped because the output switched to a low sample rate."
        }
    })
}

// MARK: Running

fn running(ui: &mut Ui, m: &mut AppModel, p: &Presentation, now: Instant) {
    // Space answers on its press only (auto-repeat is ignored); the events are taken so a
    // focused button does not also act on them.
    let space = ui.input_mut(|i| {
        let pressed = i.events.iter().any(|e| {
            matches!(
                e,
                Event::Key {
                    key: Key::Space,
                    pressed: true,
                    repeat: false,
                    ..
                }
            )
        });
        i.events.retain(|e| {
            !matches!(
                e,
                Event::Key {
                    key: Key::Space,
                    ..
                }
            )
        });
        pressed
    });
    if space {
        m.hearing_respond(now);
    }

    let total = HearingTest::TOTAL_POINTS;
    ui.horizontal(|ui| {
        let hz = grouped(p.frequency_hz as i64, i18n::lang());
        ui.label(RichText::new(format!("{hz} Hz")).size(15.0).strong());
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let c = secondary(ui.visuals());
            let count = format!("{} / {total}", p.index);
            ui.label(RichText::new(&count).monospace().color(c));
            ui.add(
                ProgressBar::new(p.index as f32 / total as f32)
                    .desired_width(160.0)
                    .desired_height(6.0),
            )
            .widget_info(|| WidgetInfo {
                current_text_value: Some(count.clone()),
                ..WidgetInfo::labeled(
                    Role::ProgressIndicator,
                    true,
                    tr("Hearing test in progress"),
                )
            });
        });
    });
    let c = secondary(ui.visuals());
    ui.label(RichText::new(tr("Press Space or click when you hear a tone.")).color(c));

    // The ear indicator and the button in the middle, Pause and Cancel at the bottom.
    let rect = ui.available_rect_before_wrap();
    let bottom_h = ui.spacing().interact_size.y;
    let middle = Rect::from_min_max(rect.min, pos2(rect.right(), rect.bottom() - bottom_h));
    let content_h = EAR_SIZE + 36.0 + 16.0 + 56.0;
    let top = middle.top() + ((middle.height() - content_h) / 2.0).max(8.0);
    let mut mid = ui.new_child(
        UiBuilder::new()
            .id_salt("hearing_running_middle")
            .max_rect(Rect::from_min_max(pos2(rect.left(), top), middle.max))
            .layout(Layout::top_down(Align::Center)),
    );
    mid.spacing_mut().item_spacing.y = 16.0;
    ear_indicator(&mut mid, Some(p.ear));
    let hear = Button::new(
        RichText::new(tr("I hear it"))
            .size(20.0)
            .color(Color32::WHITE),
    )
    .fill(theme::ACCENT)
    .min_size(vec2(240.0, 56.0));
    if mid.add(hear).clicked() {
        m.hearing_respond(now);
    }

    let bottom = Rect::from_min_max(pos2(rect.left(), rect.bottom() - bottom_h), rect.max);
    let mut row = ui.new_child(
        UiBuilder::new()
            .id_salt("hearing_running_buttons")
            .max_rect(bottom)
            .layout(Layout::right_to_left(Align::Center)),
    );
    if row.button(tr("Cancel")).clicked() {
        m.hearing_cancel(now);
    } else if row.button(tr("Pause")).clicked() {
        m.hearing_pause(now);
    }
    ui.advance_cursor_after_rect(rect);
    ui.ctx().request_repaint_after(Duration::from_millis(33));
}

/// The two ears, seen from the front as the listener faces the screen: left ear (blue) on the
/// left, right ear (red) on the right. The ear being tested is lit and pulses.
fn ear_indicator(ui: &mut Ui, active: Option<Ear>) {
    let label_h = 36.0;
    let size = vec2(2.0 * EAR_SIZE + EAR_SPACING, EAR_SIZE + label_h);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::hover());
    let name = active.map(ear_name).unwrap_or_default();
    resp.widget_info(|| WidgetInfo::labeled(Role::Image, true, &name));
    if !ui.is_rect_visible(rect) {
        return;
    }
    let t = ui.input(|i| i.time);
    let pulse = 0.75 + 0.25 * (t * TAU as f64 / PULSE_PERIOD_S).cos() as f32;
    let p = ui.painter_at(rect);
    let v = ui.visuals();
    for ear in [Ear::Left, Ear::Right] {
        let on = active == Some(ear);
        let color = theme::ear_color(ear);
        let x = if ear == Ear::Left {
            rect.left()
        } else {
            rect.right() - EAR_SIZE
        };
        let circle = Rect::from_min_size(pos2(x, rect.top()), vec2(EAR_SIZE, EAR_SIZE));
        if on {
            p.circle_filled(circle.center(), EAR_SIZE / 2.0, color.gamma_multiply(0.14));
        }
        let glyph = if on {
            color.gamma_multiply(pulse)
        } else {
            secondary(v).gamma_multiply(0.25)
        };
        paint_ear(&p, circle.center(), ear == Ear::Left, glyph);
        let text_color = if on { color } else { secondary(v) };
        let font = FontId::proportional(15.0);
        let label_pos = pos2(circle.center().x, circle.bottom() + 8.0);
        let galley = p.layout_no_wrap(ear_name(ear), font, text_color);
        let at = Align2::CENTER_TOP.anchor_size(label_pos, galley.size()).min;
        p.galley(at, galley.clone(), text_color);
        if on {
            // Semibold: the text again, a hair to the right.
            p.galley(at + vec2(0.4, 0.0), galley, text_color);
        }
    }
}

/// A simple ear: the helix as an open curve ending in the lobe, and the inner fold. Mirrored
/// for the left ear so both lobes face the middle, where the head would be.
fn paint_ear(p: &egui::Painter, c: egui::Pos2, mirror: bool, color: Color32) {
    let sx = if mirror { -1.0 } else { 1.0 };
    let arc = |cx: f32, cy: f32, rx: f32, ry: f32, from: f32, to: f32| -> Vec<egui::Pos2> {
        (0..=32)
            .map(|i| {
                let a = (from + (to - from) * i as f32 / 32.0).to_radians();
                pos2(c.x + sx * (cx + rx * a.cos()), c.y + cy + ry * a.sin())
            })
            .collect()
    };
    let mut outer = arc(0.0, -12.0, 34.0, 42.0, -200.0, 60.0);
    // The lobe: down and back in towards the head.
    outer.extend(arc(-2.0, 30.0, 16.0, 18.0, -20.0, 150.0));
    let inner = arc(2.0, -10.0, 15.0, 20.0, -170.0, 80.0);
    for (pts, w) in [(outer, 10.0), (inner, 7.0)] {
        if let (Some(a), Some(b)) = (pts.first(), pts.last()) {
            p.circle_filled(*a, w / 2.0, color);
            p.circle_filled(*b, w / 2.0, color);
        }
        p.add(Shape::line(pts, Stroke::new(w, color)));
    }
}

// MARK: Paused

fn paused(ui: &mut Ui, m: &mut AppModel, v: &View, reason: PauseReason, now: Instant) {
    ui.spacing_mut().item_spacing.y = SECTION_SPACING;
    headline(ui, &tr("Paused"));
    let mut warnings = Vec::new();
    if reason == PauseReason::OtherSound || v.others_playing {
        warnings.push(tr(
            "Another app is playing sound. Stop it, then resume the test.",
        ));
    }
    if v.muted {
        warnings.push(tr("The output is muted. Unmute it, then resume the test."));
    }
    if v.low_sample_rate {
        warnings.push(low_sample_rate_text());
    }
    warning_list(ui, &warnings);
    ui.horizontal(|ui| {
        let resume = prominent(ui, &tr("Resume"), v.can_resume).clicked();
        let cancel = ui.button(tr("Cancel")).clicked();
        if cancel {
            m.hearing_cancel(now);
        } else if resume || (v.can_resume && default_action(ui)) {
            m.hearing_resume(now);
        }
    });
}

// MARK: Result

fn result(
    ui: &mut Ui,
    m: &mut AppModel,
    v: &View,
    r: &HearingTestResult,
    form_id: Id,
    now: Instant,
) {
    ui.spacing_mut().item_spacing.y = 12.0;
    ui.horizontal(|ui| {
        headline(ui, &tr("Done"));
        let c = secondary(ui.visuals());
        ui.label(RichText::new(trf("Relative to %@", &[&v.device_name])).color(c));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            legend_entry(ui, Ear::Left);
            ui.add_space(4.0);
            legend_entry(ui, Ear::Right);
        });
    });
    audiogram::show_read_only(ui, &r.audiogram(Ear::Left), &r.audiogram(Ear::Right), 300.0);

    let mut warnings = Vec::new();
    if v.remind_volume_down {
        warnings.push(tr("Turn the volume back down now."));
    }
    warnings.extend(TestWarning::from_flags(&r.flags).iter().map(warning_text));
    warning_list(ui, &warnings);

    let c = secondary(ui.visuals());
    ui.label(
        RichText::new(tr(
            "After saving, play some audio and press Auto level to set the output level.",
        ))
        .color(c),
    );

    let mut form = ui
        .data(|d| d.get_temp::<SaveForm>(form_id))
        .unwrap_or(SaveForm {
            name: v.default_name.clone(),
            assign: true,
        });
    ui.horizontal(|ui| {
        ui.add(
            TextEdit::singleline(&mut form.name)
                .hint_text(tr("Name"))
                .desired_width(320.0),
        )
        .accessible_name(tr("Name"));
        ui.checkbox(&mut form.assign, trf("Use on %@", &[&v.device_name]));
    });
    ui.data_mut(|d| d.insert_temp(form_id, form.clone()));

    ui.horizontal(|ui| {
        let failed = v.phase == Phase::SaveFailed;
        if failed {
            warning(ui, &tr("The profile could not be saved."));
        }
        let save = prominent(ui, &tr(if failed { "Retry" } else { "Save" }), true).clicked();
        let discard = ui.button(tr("Discard")).clicked();
        if discard {
            m.hearing_discard(now);
        } else if save || (!failed && default_action(ui)) {
            m.hearing_save(&form.name, form.assign);
        }
    });
}

fn warning_text(w: &TestWarning) -> String {
    let list = |fs: &[f64]| {
        fs.iter()
            .map(|f| (*f as i64).to_string())
            .collect::<Vec<_>>()
            .join(", ")
    };
    match w {
        TestWarning::RetestMismatch { ear, db } => {
            let d = (db.round() as i64).to_string();
            trf(
                match ear {
                    Ear::Left => {
                        "The 1 kHz retest differed by %@ dB in the left ear — consider testing again."
                    }
                    Ear::Right => {
                        "The 1 kHz retest differed by %@ dB in the right ear — consider testing again."
                    }
                },
                &[&d],
            )
        }
        TestWarning::CatchTrialPresses => {
            tr("You may be pressing without hearing a tone — consider testing again.")
        }
        TestWarning::NoResponseAtMax {
            ear,
            frequencies_hz,
        } => trf(
            match ear {
                Ear::Left => {
                    "No response at the maximum level at %@ Hz (left ear): the real threshold may be higher."
                }
                Ear::Right => {
                    "No response at the maximum level at %@ Hz (right ear): the real threshold may be higher."
                }
            },
            &[&list(frequencies_hz)],
        ),
        TestWarning::NotConverged {
            ear,
            frequencies_hz,
        } => trf(
            match (ear, frequencies_hz.len() > 1) {
                (Ear::Left, false) => "The %@ Hz result (left ear) is uncertain.",
                (Ear::Right, false) => "The %@ Hz result (right ear) is uncertain.",
                (Ear::Left, true) => "The %@ Hz results (left ear) are uncertain.",
                (Ear::Right, true) => "The %@ Hz results (right ear) are uncertain.",
            },
            &[&list(frequencies_hz)],
        ),
        TestWarning::Asymmetry => {
            tr("The worse ear may be underestimated. An audiologist can measure it with masking.")
        }
    }
}

/// "○ Right ear" or "× Left ear", the marker in the ear's colour.
fn legend_entry(ui: &mut Ui, ear: Ear) {
    ui.label(RichText::new(ear_name(ear)).size(13.0));
    let (rect, _) = ui.allocate_exact_size(vec2(12.0, 12.0), Sense::hover());
    let (c, r) = (rect.center(), 4.5);
    let s = Stroke::new(1.5, theme::ear_color(ear));
    match ear {
        Ear::Left => {
            ui.painter()
                .line_segment([c - vec2(r, r), c + vec2(r, r)], s);
            ui.painter()
                .line_segment([c + vec2(-r, r), c + vec2(r, -r)], s);
        }
        Ear::Right => {
            ui.painter().circle_stroke(c, r, s);
        }
    }
}

// MARK: Shared

fn low_sample_rate_text() -> String {
    tr(
        "The output runs at a low sample rate (for example a headset in call mode). Stop using its microphone and try again.",
    )
}

fn ear_name(ear: Ear) -> String {
    tr(match ear {
        Ear::Left => "Left ear",
        Ear::Right => "Right ear",
    })
}

fn headline(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).size(15.0).strong());
}

fn bullet(ui: &mut Ui, text: &str) {
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        ui.label("•");
        ui.add(Label::new(text).wrap());
    });
}

fn warning_list(ui: &mut Ui, warnings: &[String]) {
    if warnings.is_empty() {
        return;
    }
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = 6.0;
        ui.vertical(|ui| {
            for w in warnings {
                warning(ui, w);
            }
        });
    });
}

/// An orange warning triangle and the text.
fn warning(ui: &mut Ui, text: &str) {
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        let (rect, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
        let color = theme::warning(ui.visuals());
        let (l, r, t, b) = (
            rect.left() + 1.0,
            rect.right() - 1.0,
            rect.top() + 1.5,
            rect.bottom() - 1.0,
        );
        ui.painter().add(Shape::convex_polygon(
            vec![pos2(rect.center().x, t), pos2(r, b), pos2(l, b)],
            color,
            Stroke::NONE,
        ));
        ui.painter().text(
            pos2(rect.center().x, rect.center().y + 1.5),
            Align2::CENTER_CENTER,
            "!",
            FontId::proportional(11.0),
            Color32::WHITE,
        );
        ui.add(Label::new(text).wrap());
    });
}

/// The default button: accent fill and white text.
fn prominent(ui: &mut Ui, text: &str, enabled: bool) -> Response {
    let b = Button::new(RichText::new(text).color(Color32::WHITE)).fill(theme::ACCENT);
    ui.add_enabled(enabled, b)
}

/// Return pressed with no widget focused, or in a text field it ends: the default button.
fn default_action(ui: &Ui) -> bool {
    ui.memory(|m| m.focused().is_none()) && ui.input(|i| i.key_pressed(Key::Enter))
}

/// An integer with the language's thousands separator, as Swift's `Int.formatted()`.
fn grouped(n: i64, lang: Lang) -> String {
    let digits = n.unsigned_abs().to_string();
    let sep = match lang {
        Lang::En => ",",
        Lang::Da | Lang::De => ".",
        Lang::Sv | Lang::Nb | Lang::Fi => "\u{a0}",
        Lang::Fr => "\u{202f}",
        // Spanish groups only from five digits.
        Lang::Es if digits.len() <= 4 => "",
        Lang::Es => ".",
    };
    let mut out = String::new();
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push_str(sep);
        }
        out.push(ch);
    }
    if n < 0 { format!("-{out}") } else { out }
}
