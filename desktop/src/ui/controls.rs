//! The controls panel beside the plot (Swift `ControlsPanel`): mode, the level sliders,
//! Compression and Limiter, the "Active on" dropdown, Auto level and the meters.

use std::ops::RangeInclusive;
use std::time::{Duration, Instant};

use audiocurve_core::fitting::CompressionMode;
use audiocurve_dsp::Meters;
use egui::{
    Align, Align2, Button, Checkbox, FontId, Id, Label, Layout, Popup, PopupCloseBehavior, Rect,
    RichText, Role, ScrollArea, Sense, Ui, WidgetInfo, pos2, vec2,
};

use crate::i18n::{tr, trf};
use crate::model::app::AppModel;
use crate::model::auto_level::{AutoLevelUi, BLOCK_INTERVAL};
use crate::model::editing_input::fmt;
use crate::model::levels::{INPUT_GAIN_RANGE, MPO_RANGE, OUTPUT_GAIN_RANGE};
use crate::ui::theme;
use crate::ui::widgets::{
    ModalAnswer, confirm_modal, labeled_slider, meter_pair, primary, secondary, segmented,
};

/// Width of the panel, frame included.
pub const WIDTH: f32 = 232.0;
const METER_LABEL_WIDTH: f32 = 48.0;
const METER_HEIGHT: f32 = 14.0;
const REDUCTION_MAX_DB: f32 = 24.0;
const REDUCTION_VALUE_WIDTH: f32 = 56.0;
/// The meters redraw 20 times a second.
const REFRESH: Duration = Duration::from_millis(50);

fn small(text: impl Into<String>) -> RichText {
    RichText::new(text).size(13.0)
}

pub fn show(ui: &mut Ui, m: &mut AppModel, now: Instant) {
    let meters = m.engine().meters();
    let height = ui.available_height();
    theme::panel_frame(ui.style()).show(ui, |ui| {
        let inner_height = (height - 2.0 * 14.0).max(0.0);
        ui.set_width(WIDTH - 2.0 * 14.0);
        ui.set_min_height(inner_height);
        // Scrolls when the window is too short for the panel, so nothing is cut off.
        ScrollArea::vertical()
            .max_height(inner_height)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 8.0;
                let top = ui.cursor().top();

                mode(ui, m, &meters);
                sliders(ui, m);
                toggles(ui, m);
                active_outputs(ui, m, now);
                if !m.levels.limiter {
                    ui.label(
                        small(tr("Limiter off: no level ceiling below 0 dBFS"))
                            .color(theme::warning(ui.visuals())),
                    );
                }
                auto_level(ui, m, now);

                // The meters sit at the bottom; their height is the one measured last frame.
                let id = ui.scope_id().with("meters_height");
                let block = ui.data(|d| d.get_temp::<f32>(id)).unwrap_or(90.0);
                let used = ui.cursor().top() - top;
                ui.add_space((inner_height - used - block).max(0.0));
                let r = ui.vertical(|ui| meters_block(ui, m, &meters)).response;
                ui.data_mut(|d| d.insert_temp(id, r.rect.height()));
            });
    });
    ui.ctx().request_repaint_after(REFRESH);
}

fn mode(ui: &mut Ui, m: &mut AppModel, meters: &Meters) {
    let (speech, music, auto) = (tr("Speech"), tr("Music"), tr("Auto"));
    segmented(
        ui,
        &mut m.levels.mode,
        &[
            (CompressionMode::Speech, speech.as_str()),
            (CompressionMode::Music, music.as_str()),
            (CompressionMode::Auto, auto.as_str()),
        ],
    );
    if m.levels.mode == CompressionMode::Auto {
        // Same hysteresis idea as the kernel: the label only names a side once it is clear.
        let s = meters.speech;
        let text = if s > 0.7 {
            tr("Auto: speech")
        } else if s < 0.3 {
            tr("Auto: music")
        } else {
            tr("Auto: listening…")
        };
        ui.label(small(text).color(secondary(ui.visuals())));
    }
}

/// A slider row with Swift's step and a double click back to the default.
fn slider(
    ui: &mut Ui,
    label: &str,
    value: &mut f64,
    range: RangeInclusive<f64>,
    step: f64,
    default: f64,
    format: impl Fn(f64) -> String,
) -> egui::Response {
    let text = format(*value);
    let r = labeled_slider(ui, label, value, range, &text);
    if r.changed() {
        *value = (*value / step).round() * step;
    }
    if r.double_clicked() {
        *value = default;
    }
    r
}

fn sliders(ui: &mut Ui, m: &mut AppModel) {
    let percent = |v: f64| trf("%lld %%", &[&(v.round() as i64).to_string()]);
    let db = |v: f64| format!("{} dB", fmt::signed(v));
    let l = &mut m.levels;
    slider(
        ui,
        &tr("Strength"),
        &mut l.strength,
        0.0..=100.0,
        1.0,
        50.0,
        percent,
    );
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        slider(ui, &tr("Tone"), &mut l.tone, 0.0..=100.0, 1.0, 0.0, percent);
        ui.horizontal(|ui| {
            let c = secondary(ui.visuals());
            ui.label(RichText::new(tr("Clear")).size(11.0).color(c));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(RichText::new(tr("Warm")).size(11.0).color(c));
            });
        });
    });
    slider(
        ui,
        &tr("Input level"),
        &mut l.input_gain_db,
        INPUT_GAIN_RANGE,
        0.5,
        0.0,
        db,
    )
    .on_hover_text(tr(
        "Lowers or raises the sound before it is amplified. A lower input level makes the compression work less.",
    ));
    slider(
        ui,
        &tr("Output level"),
        &mut l.output_gain_db,
        OUTPUT_GAIN_RANGE,
        0.5,
        0.0,
        db,
    );
    slider(
        ui,
        &tr("Ceiling (MPO)"),
        &mut l.mpo_dbfs,
        MPO_RANGE,
        1.0,
        -3.0,
        |v| format!("{} dBFS", fmt::db(v)),
    );
}

/// Compression and Limiter. Turning the limiter off asks first; turning it on never does.
fn toggles(ui: &mut Ui, m: &mut AppModel) {
    let confirm_id = Id::unique("confirm_limiter_off");
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 16.0;
        ui.checkbox(&mut m.levels.compression, small(tr("Compression")));
        let mut on = m.levels.limiter;
        if ui.checkbox(&mut on, small(tr("Limiter"))).changed() {
            if on {
                m.levels.limiter = true;
            } else {
                ui.data_mut(|d| d.insert_temp(confirm_id, true));
            }
        }
    });
    if !ui.data(|d| d.get_temp::<bool>(confirm_id).unwrap_or(false)) {
        return;
    }
    let answer = confirm_modal(
        ui.ctx(),
        confirm_id.with("modal"),
        &tr("Turn off the limiter?"),
        Some(&tr(
            "Without the limiter there is no level ceiling below 0 dBFS. With strong amplification the sound can get very loud and damage your hearing. Lower the volume before turning it off.",
        )),
        &tr("Turn Off"),
    );
    if answer != ModalAnswer::Open {
        if answer == ModalAnswer::Confirmed {
            m.levels.limiter = false;
        }
        ui.data_mut(|d| d.remove::<bool>(confirm_id));
    }
}

fn active_summary(m: &AppModel) -> String {
    let a = &m.active_outputs;
    if a.is_all() {
        tr("Active on: all outputs")
    } else if a.devices.len() == 1 {
        trf("Active on: %@", &[&a.devices[0].name])
    } else {
        trf("Active on: %lld outputs", &[&a.devices.len().to_string()])
    }
}

/// "Active on": a dropdown with a checkbox per output (several can be ticked in one go), and a
/// notice while the current output is not one of them.
fn active_outputs(ui: &mut Ui, m: &mut AppModel, now: Instant) {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 4.0;
        let button = Button::new(small(active_summary(m)))
            .truncate()
            .min_size(vec2(ui.available_width(), 0.0));
        let resp = ui.add(button).on_hover_text(tr(
            "Choose the output devices AudioCurve processes sound for. On other outputs the sound passes through unchanged.",
        ));
        Popup::from_toggle_button_response(&resp)
            .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
            .show(|ui| output_checkboxes(ui, m, now));

        let current = m.current_output.as_ref();
        if !m
            .active_outputs
            .is_active(current.map(|d| d.uid.as_str()))
        {
            let name = current.map_or("", |d| d.name.as_str());
            ui.add(
                Label::new(
                    small(trf("Paused – %@ is the output", &[name]))
                        .color(theme::warning(ui.visuals())),
                )
                .truncate(),
            );
        }
    });
}

fn output_checkboxes(ui: &mut Ui, m: &mut AppModel, now: Instant) {
    let current_uid = m.current_output.as_ref().map(|d| d.uid.clone());
    for d in m.active_outputs.menu_devices(&m.available_outputs) {
        let connected = m.available_outputs.iter().any(|o| o.uid == d.uid);
        let label = if current_uid.as_deref() == Some(d.uid.as_str()) {
            format!("{} {}", d.name, tr("(current)"))
        } else if !connected {
            format!("{} {}", d.name, tr("(not connected)"))
        } else {
            d.name.clone()
        };
        let mut checked = m.active_outputs.is_checked(&d.uid);
        let enabled = !m
            .active_outputs
            .is_last_checked(&d.uid, &m.available_outputs);
        if ui
            .add_enabled(enabled, Checkbox::new(&mut checked, small(label)))
            .changed()
        {
            m.set_output_enabled(&d, checked, now);
        }
    }
}

fn auto_level(ui: &mut Ui, m: &mut AppModel, now: Instant) {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 4.0;
        let state = *m.auto_level();
        let measuring = matches!(state, AutoLevelUi::Measuring { .. });
        let button = Button::new(tr("Auto level")).min_size(vec2(ui.available_width(), 28.0));
        if ui.add_enabled(!measuring, button).clicked() {
            m.start_auto_level(now);
        }
        let v = ui.visuals();
        let (text, color) = match state {
            AutoLevelUi::Idle => (String::new(), secondary(v)),
            AutoLevelUi::Measuring { remaining } => {
                let secs = (remaining as f64 * BLOCK_INTERVAL.as_secs_f64()).ceil() as i64;
                (trf("Measuring… %lld s", &[&secs.to_string()]), secondary(v))
            }
            AutoLevelUi::Done(g) => (
                trf("Output level set to %@ dB", &[&fmt::signed(g)]),
                secondary(v),
            ),
            AutoLevelUi::TooQuiet => (tr("Too quiet – play something"), theme::warning(v)),
        };
        // Always one line high, so the panel does not jump.
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 16.0), Sense::hover());
        if !text.is_empty() {
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect));
            child.add(Label::new(small(text).color(color)).truncate());
        }
    });
}

fn meters_block(ui: &mut Ui, m: &AppModel, meters: &Meters) {
    ui.spacing_mut().item_spacing.y = 6.0;
    // The rows are as high as the bars, not a button.
    ui.spacing_mut().interact_size.y = METER_HEIGHT;
    meter_pair(
        ui,
        &tr("In"),
        meters.in_rms_db[0],
        meters.in_rms_db[1],
        Some(meters.in_peak_db),
    );
    meter_pair(
        ui,
        &tr("Out"),
        meters.out_rms_db[0],
        meters.out_rms_db[1],
        Some(meters.out_peak_db),
    );
    // The scale under the bars: −60, −30, 0 dBFS.
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 13.0), Sense::hover());
    let scale = Rect::from_min_max(
        pos2(rect.left() + METER_LABEL_WIDTH + 8.0, rect.top()),
        rect.max,
    );
    let p = ui.painter();
    let (font, c) = (FontId::proportional(11.0), secondary(ui.visuals()));
    p.text(
        scale.left_center(),
        Align2::LEFT_CENTER,
        "\u{2212}60",
        font.clone(),
        c,
    );
    p.text(
        scale.center(),
        Align2::CENTER_CENTER,
        "\u{2212}30",
        font.clone(),
        c,
    );
    p.text(
        scale.right_center(),
        Align2::RIGHT_CENTER,
        "0 dBFS",
        font,
        c,
    );
    ui.add_space(4.0);
    reduction_meter(ui, meters.reduction_db, m.levels.limiter);
}

/// "−N" for a reduction of at least 0.5 dB, else "0".
fn reduction_text(db: f32) -> String {
    if db.is_finite() && db >= 0.5 {
        format!("\u{2212}{}", db.round() as i64)
    } else {
        "0".into()
    }
}

/// How much the safety stage lowers each ear right now, 0…24 dB (Swift `ReductionMeterView`).
fn reduction_meter(ui: &mut Ui, reduction: [f32; 2], enabled: bool) {
    let r = ui
        .horizontal(|ui| {
            let (label_rect, _) =
                ui.allocate_exact_size(vec2(METER_LABEL_WIDTH, METER_HEIGHT), Sense::hover());
            let text_color = ui.visuals().text_color();
            ui.painter().with_clip_rect(label_rect).text(
                label_rect.left_center(),
                Align2::LEFT_CENTER,
                tr("Safety"),
                FontId::proportional(13.0),
                text_color,
            );
            let width = (ui.available_width() - REDUCTION_VALUE_WIDTH - 8.0).max(0.0);
            let (rect, resp) = ui.allocate_exact_size(vec2(width, METER_HEIGHT), Sense::hover());
            let value = if enabled {
                trf(
                    "Left %@ dB, right %@ dB",
                    &[&reduction_text(reduction[0]), &reduction_text(reduction[1])],
                )
            } else {
                tr("Limiter off")
            };
            resp.widget_info(|| WidgetInfo {
                current_text_value: Some(value.clone()),
                ..WidgetInfo::labeled(Role::Meter, true, tr("Safety, reduction"))
            });
            if ui.is_rect_visible(rect) {
                let p = ui.painter_at(rect);
                let track = primary(ui.visuals(), 0.08);
                let row_h = ((rect.height() - 2.0) / 2.0).max(0.0);
                for (i, (db, color)) in [(reduction[0], theme::LEFT), (reduction[1], theme::RIGHT)]
                    .into_iter()
                    .enumerate()
                {
                    let y = rect.top() + i as f32 * (row_h + 2.0);
                    let row = Rect::from_min_size(pos2(rect.left(), y), vec2(rect.width(), row_h));
                    p.rect_filled(row, 2.0, track);
                    let fraction = if db.is_finite() {
                        (db / REDUCTION_MAX_DB).clamp(0.0, 1.0)
                    } else {
                        0.0
                    };
                    let w = rect.width() * fraction;
                    if enabled && w > 0.5 {
                        let bar = Rect::from_min_size(row.min, vec2(w, row_h));
                        p.rect_filled(bar, 2.0, color.gamma_multiply(0.85));
                    }
                }
            }
            let text = if enabled {
                format!("{} dB", reduction_text(reduction[0].max(reduction[1])))
            } else {
                tr("off")
            };
            let (value_rect, _) =
                ui.allocate_exact_size(vec2(REDUCTION_VALUE_WIDTH, METER_HEIGHT), Sense::hover());
            ui.painter().text(
                value_rect.right_center(),
                Align2::RIGHT_CENTER,
                text,
                FontId::proportional(13.0),
                secondary(ui.visuals()),
            );
        })
        .response;
    r.on_hover_text(tr(
        "How much the limiter is lowering the sound right now. If it often shows more than a few dB, lower Output level and turn up the computer volume instead.",
    ));
}
