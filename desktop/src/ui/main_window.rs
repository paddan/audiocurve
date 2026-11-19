//! The main window (Swift `HostView` and `PluginView`): Bypass, Hearing test… and the status
//! line on top; below it the plot with its header and legend, and the controls panel. While a
//! hearing test session is open it shows the hearing test instead of the plot and controls.

use std::time::Instant;

use audiocurve_core::profile::Ear;
use audiocurve_core::route::{RouteFailure, RouteTarget};
use egui::{
    Align, Button, Color32, CornerRadius, Frame, Label, Layout, Rect, RichText, Sense, Shape,
    Stroke, Ui, UiBuilder, pos2, vec2,
};

use crate::i18n::tr;
use crate::model::app::AppModel;
use crate::model::editing_input::fmt;
use crate::model::editor::Pane;
use crate::model::status::{Tone, status_text};
use crate::ui::spectrum::SpectrumEars;
use crate::ui::widgets::{paint_card, secondary, segmented};
use crate::ui::{audiogram, controls, gain, hearing_test, profile_menu, spectrum, theme};

/// The smallest inner size of the main window (Swift `HostView`); everything fits at it.
pub const MIN_WINDOW_SIZE: [f32; 2] = [720.0, 700.0];
/// Space between the plot column and the controls panel.
const COLUMN_GAP: f32 = 16.0;
const LEGEND_HEIGHT: f32 = 26.0;
const PANE_PICKER_WIDTH: f32 = 280.0;

pub fn show(ui: &mut Ui, m: &mut AppModel, now: Instant) {
    let testing = m.hearing().is_some();
    ui.horizontal(|ui| {
        let mut bypass = m.bypass;
        if ui.checkbox(&mut bypass, tr("Bypass (A/B)")).changed() {
            m.set_bypass(bypass, now);
        }
        if ui
            .add_enabled(!testing, Button::new(tr("Hearing test…")))
            .clicked()
        {
            m.open_hearing_test(now);
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            status_line(ui, m, now)
        });
    });
    ui.add_space(6.0);
    if testing {
        hearing_test::show(ui, m, now);
    } else {
        plugin_view(ui, m, now);
    }
}

/// The route state in one line, and "Try Again" while the engine failure is latched. Laid out
/// right to left.
fn status_line(ui: &mut Ui, m: &mut AppModel, now: Instant) {
    if *m.route_state() == RouteTarget::Failed(RouteFailure::Plugin)
        && ui.button(tr("Try Again")).clicked()
    {
        m.retry_engine(now);
    }
    let (text, tone) = status_text(m);
    let v = ui.visuals();
    let color = match tone {
        Tone::Positive => theme::positive(v),
        Tone::Warning => theme::warning(v),
        Tone::Neutral => secondary(v),
    };
    ui.add(Label::new(RichText::new(text).size(13.0).color(color)).truncate());
}

/// The plot column on the left, the controls panel on the right.
fn plugin_view(ui: &mut Ui, m: &mut AppModel, now: Instant) {
    let rect = ui.available_rect_before_wrap();
    let split = rect.right() - controls::WIDTH;
    let left = Rect::from_min_max(rect.min, pos2(split - COLUMN_GAP, rect.bottom()));
    let right = Rect::from_min_max(pos2(split, rect.top()), rect.max);
    let mut col = ui.new_child(UiBuilder::new().id_salt("plot_column").max_rect(left));
    plot_column(&mut col, m, now);
    let mut panel = ui.new_child(UiBuilder::new().id_salt("controls").max_rect(right));
    controls::show(&mut panel, m, now);
    ui.advance_cursor_after_rect(rect);
}

fn plot_column(ui: &mut Ui, m: &mut AppModel, now: Instant) {
    ui.spacing_mut().item_spacing.y = 12.0;
    header(ui, m, now);
    let chart_height =
        (ui.available_height() - LEGEND_HEIGHT - ui.spacing().item_spacing.y).max(0.0);
    let (chart, _) =
        ui.allocate_exact_size(vec2(ui.available_width(), chart_height), Sense::hover());
    let mut cui = ui.new_child(UiBuilder::new().id_salt("chart").max_rect(chart));
    plot(&mut cui, m, now);
    if let Some(message) = m.editor.error.clone() {
        error_pill(ui, chart, &message);
    }
    let (legend_rect, _) =
        ui.allocate_exact_size(vec2(ui.available_width(), LEGEND_HEIGHT), Sense::hover());
    let mut lui = ui.new_child(
        UiBuilder::new()
            .id_salt("legend")
            .max_rect(legend_rect)
            .layout(Layout::left_to_right(Align::Center)),
    );
    if m.editor.pane == Pane::Spectrum {
        spectrum_legend(&mut lui);
    } else {
        ear_legend(&mut lui, m);
    }
}

/// The pane picker, the profile menu and the plot's caption.
fn header(ui: &mut Ui, m: &mut AppModel, now: Instant) {
    ui.horizontal(|ui| {
        let (a, g, s) = (tr("Audiogram"), tr("Gain"), tr("Spectrum"));
        ui.scope(|ui| {
            ui.set_max_width(PANE_PICKER_WIDTH);
            segmented(
                ui,
                &mut m.editor.pane,
                &[
                    (Pane::Audiogram, a.as_str()),
                    (Pane::Gain, g.as_str()),
                    (Pane::Spectrum, s.as_str()),
                ],
            );
        });
        let has_profile = m.editor.profile.is_some();
        if has_profile {
            profile_menu::show(ui, m, now);
        }
        let pane = m.editor.pane;
        if has_profile || pane == Pane::Spectrum {
            let caption = match pane {
                Pane::Audiogram => tr("Hearing threshold, dB HL"),
                Pane::Gain => tr("Gain per band, dB"),
                Pane::Spectrum => tr("Spectrum, dBFS"),
            };
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let c = secondary(ui.visuals());
                ui.add(Label::new(RichText::new(caption).size(13.0).color(c)).truncate());
            });
        }
    });
}

fn plot(ui: &mut Ui, m: &mut AppModel, now: Instant) {
    match m.editor.pane {
        Pane::Spectrum => {
            let id = ui.scope_id().with("spectrum_ears");
            let mut ears = ui
                .data(|d| d.get_temp::<SpectrumEars>(id))
                .unwrap_or_default();
            spectrum::show(ui, m, &mut ears);
            ui.data_mut(|d| d.insert_temp(id, ears));
        }
        _ if m.editor.profile.is_none() => empty_state(ui, m),
        Pane::Audiogram => audiogram::show(ui, &mut m.editor, now),
        Pane::Gain => gain::show(ui, m, now),
    }
}

/// "No audiogram" and "Add audiogram" on an empty chart card.
fn empty_state(ui: &mut Ui, m: &mut AppModel) {
    let rect = ui.max_rect();
    paint_card(ui.painter(), rect, ui.visuals());
    let mut inner = ui.new_child(
        UiBuilder::new()
            .max_rect(rect)
            .layout(Layout::top_down(Align::Center)),
    );
    inner.add_space((rect.height() / 2.0 - 40.0).max(0.0));
    let c = secondary(inner.visuals());
    inner.label(
        RichText::new(tr("No audiogram"))
            .size(15.0)
            .strong()
            .color(c),
    );
    let add = Button::new(RichText::new(tr("Add audiogram")).color(Color32::WHITE))
        .fill(theme::ACCENT)
        .min_size(vec2(0.0, 30.0));
    if inner.add(add).clicked() {
        m.editor.add_flat_profile();
    }
}

/// The editor's error as a red capsule near the bottom of the chart.
fn error_pill(ui: &mut Ui, chart: Rect, message: &str) {
    let area = Rect::from_min_max(chart.min, pos2(chart.right(), chart.bottom() - 32.0));
    let mut child = ui.new_child(
        UiBuilder::new()
            .id_salt("error_pill")
            .max_rect(area)
            .layout(Layout::bottom_up(Align::Center)),
    );
    Frame::new()
        .fill(theme::ALERT)
        .corner_radius(CornerRadius::same(14))
        .inner_margin(egui::Margin::symmetric(12, 6))
        .show(&mut child, |ui| {
            ui.label(
                RichText::new(message)
                    .size(13.0)
                    .strong()
                    .color(Color32::WHITE),
            );
        });
}

/// The active-ear picker and, on the gain pane, the key of the bars and ticks.
fn ear_legend(ui: &mut Ui, m: &mut AppModel) {
    let (l, r) = (fmt::ear_name(Ear::Left), fmt::ear_name(Ear::Right));
    segmented(
        ui,
        &mut m.editor.active_ear,
        &[(Ear::Left, l.as_str()), (Ear::Right, r.as_str())],
    );
    if m.editor.profile.is_none() || m.editor.pane != Pane::Gain {
        return;
    }
    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        let c = secondary(ui.visuals());
        ui.label(RichText::new(tr("Music")).size(13.0).color(c));
        swatch(ui, false);
        ui.label(RichText::new(tr("Speech")).size(13.0).color(c));
        swatch(ui, true);
        ui.add(
            Label::new(
                RichText::new(tr("Bar: soft · dashed: loud"))
                    .size(13.0)
                    .color(c),
            )
            .truncate(),
        );
    });
}

/// A short tick sample: solid for speech, dotted for music, as drawn in the gain chart.
fn swatch(ui: &mut Ui, solid: bool) {
    let (rect, _) = ui.allocate_exact_size(vec2(14.0, 6.0), Sense::hover());
    let stroke = Stroke::new(1.5, secondary(ui.visuals()));
    let (a, b) = (rect.left_center(), rect.right_center());
    if solid {
        ui.painter().line_segment([a, b], stroke);
    } else {
        ui.painter()
            .extend(Shape::dotted_line(&[a, b], stroke.color, 3.0, 0.9));
    }
}

/// "▒ In   — Out": the key of the spectrum.
fn spectrum_legend(ui: &mut Ui) {
    let c = secondary(ui.visuals());
    let (r, _) = ui.allocate_exact_size(vec2(14.0, 10.0), Sense::hover());
    ui.painter().rect_filled(r, 2.0, c.gamma_multiply(0.3));
    ui.label(RichText::new(tr("In")).size(13.0).color(c));
    ui.add_space(8.0);
    for color in [theme::LEFT, theme::RIGHT] {
        let (r, _) = ui.allocate_exact_size(vec2(14.0, 2.5), Sense::hover());
        ui.painter().rect_filled(r, 1.25, color);
    }
    ui.label(RichText::new(tr("Out")).size(13.0).color(c))
        .on_hover_text(tr("Input is the grey area, output is the line"));
}
