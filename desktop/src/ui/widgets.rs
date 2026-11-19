//! Small controls in the tuned look (segmented control, labelled slider, level meter) and the
//! painting helpers the plots share.

use std::ops::RangeInclusive;

use egui::{
    Align, Align2, Button, Color32, Context, CornerRadius, FontId, Frame, Id, Layout, Modal,
    Painter, Pos2, Rect, Response, RichText, Role, Sense, Shape, Slider, Stroke, StrokeKind,
    TextStyle, Ui, Visuals, WidgetInfo, WidgetText, pos2, vec2,
};

use crate::i18n::tr;
use crate::ui::theme;

/// Rounding of the segmented control's track.
const TRACK_RADIUS: u8 = 7;
const TRACK_MARGIN: f32 = 2.0;

/// A segmented picker: a rounded track in the faint background colour, the selected segment
/// filled with the accent and white text. All segments are as wide as the widest. The response
/// is the union of the segments, marked changed when the value changed.
pub fn segmented<T: PartialEq + Copy>(ui: &mut Ui, value: &mut T, items: &[(T, &str)]) -> Response {
    let padding = ui.spacing().button_padding;
    let height = (ui.spacing().interact_size.y - 2.0 * TRACK_MARGIN).max(0.0);
    let galleys: Vec<_> = items
        .iter()
        .map(|(_, label)| {
            WidgetText::from(*label).into_galley(
                ui,
                Some(egui::TextWrapMode::Extend),
                f32::INFINITY,
                TextStyle::Button,
            )
        })
        .collect();
    let width = galleys.iter().map(|g| g.size().x).fold(0.0, f32::max) + 2.0 * padding.x;
    Frame::new()
        .fill(ui.visuals().faint_bg_color)
        .corner_radius(TRACK_RADIUS)
        .inner_margin(TRACK_MARGIN)
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.x = TRACK_MARGIN;
            ui.horizontal(|ui| {
                let mut union: Option<Response> = None;
                for ((item, label), galley) in items.iter().zip(galleys) {
                    let selected = *value == *item;
                    let (rect, mut r) = ui.allocate_exact_size(vec2(width, height), Sense::click());
                    if r.clicked() && !selected {
                        *value = *item;
                        r.mark_changed();
                    }
                    let selected = *value == *item;
                    r.widget_info(|| {
                        WidgetInfo::selected(Role::RadioButton, true, selected, label)
                    });
                    if ui.is_rect_visible(rect) {
                        let v = ui.visuals();
                        let radius = CornerRadius::from(TRACK_RADIUS - TRACK_MARGIN as u8);
                        let text = if selected {
                            ui.painter().rect_filled(rect, radius, v.selection.bg_fill);
                            Color32::WHITE
                        } else {
                            if r.hovered() {
                                ui.painter().rect_filled(
                                    rect,
                                    radius,
                                    v.widgets.hovered.weak_bg_fill,
                                );
                            }
                            v.text_color()
                        };
                        let pos = rect.center() - galley.size() / 2.0;
                        ui.painter().galley(pos, galley, text);
                    }
                    union = Some(match union {
                        Some(u) => u | r,
                        None => r,
                    });
                }
                union.unwrap_or_else(|| ui.allocate_response(vec2(0.0, 0.0), Sense::hover()))
            })
            .inner
        })
        .inner
}

/// A label with its value text on the right, over a full-width slider. The slider carries the
/// label as its accessible name.
pub fn labeled_slider(
    ui: &mut Ui,
    label: &str,
    value: &mut f64,
    range: RangeInclusive<f64>,
    value_text: &str,
) -> Response {
    ui.vertical(|ui| {
        ui.horizontal(|ui| {
            ui.label(label);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(value_text)
            });
        });
        ui.spacing_mut().slider_width = ui.available_width().max(0.0);
        ui.add(Slider::new(value, range).show_value(false))
            .accessible_name(label)
    })
    .inner
}

/// What the user did in a modal this frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModalAnswer {
    Open,
    Confirmed,
    Cancelled,
}

/// Width of the text in modals.
pub const MODAL_WIDTH: f32 = 320.0;

/// A confirmation in an `egui::Modal` (SwiftUI `.alert` with a destructive button): the title,
/// an optional message, Cancel and the confirm button. Escape or a click outside cancels.
pub fn confirm_modal(
    ctx: &Context,
    id: Id,
    title: &str,
    message: Option<&str>,
    confirm: &str,
) -> ModalAnswer {
    let r = Modal::new(id).show(ctx, |ui| {
        ui.set_max_width(MODAL_WIDTH);
        ui.label(RichText::new(title).strong());
        if let Some(message) = message {
            ui.label(message);
        }
        ui.add_space(6.0);
        let mut answer = ModalAnswer::Open;
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let destructive =
                Button::new(RichText::new(confirm).color(Color32::WHITE)).fill(theme::ALERT);
            if ui.add(destructive).clicked() {
                answer = ModalAnswer::Confirmed;
            }
            if ui.button(tr("Cancel")).clicked() {
                answer = ModalAnswer::Cancelled;
            }
        });
        answer
    });
    if r.inner == ModalAnswer::Open && r.should_close() {
        ModalAnswer::Cancelled
    } else {
        r.inner
    }
}

const METER_FLOOR_DB: f32 = -60.0;
const METER_LABEL_WIDTH: f32 = 48.0;
const METER_HEIGHT: f32 = 14.0;

/// Stereo level meter (Swift `MeterView`): a bar per ear, −60…0 dBFS, top row left (blue),
/// bottom row right (red), and a peak tick per row when `peak_db` is given.
pub fn meter_pair(
    ui: &mut Ui,
    label: &str,
    left_db: f32,
    right_db: f32,
    peak_db: Option<[f32; 2]>,
) {
    ui.horizontal(|ui| {
        let (label_rect, _) =
            ui.allocate_exact_size(vec2(METER_LABEL_WIDTH, METER_HEIGHT), Sense::hover());
        let text_color = ui.visuals().text_color();
        ui.painter().with_clip_rect(label_rect).text(
            label_rect.left_center(),
            Align2::LEFT_CENTER,
            label,
            FontId::proportional(13.0),
            text_color,
        );
        let width = ui.available_width().max(0.0);
        let (rect, resp) = ui.allocate_exact_size(vec2(width, METER_HEIGHT), Sense::hover());
        let loudest = [left_db, right_db]
            .into_iter()
            .filter(|v| v.is_finite())
            .reduce(f32::max);
        resp.widget_info(|| WidgetInfo {
            current_text_value: loudest.map(|v| format!("{} dBFS", v.round() as i64)),
            ..WidgetInfo::labeled(Role::Meter, true, label)
        });
        if !ui.is_rect_visible(rect) {
            return;
        }
        let p = ui.painter_at(rect);
        let track = primary(ui.visuals(), 0.08);
        let fg = primary(ui.visuals(), 1.0);
        let row_h = ((rect.height() - 2.0) / 2.0).max(0.0);
        let rows = [(left_db, theme::LEFT, 0), (right_db, theme::RIGHT, 1)];
        for (rms, color, i) in rows {
            let y = rect.top() + i as f32 * (row_h + 2.0);
            let row = Rect::from_min_size(pos2(rect.left(), y), vec2(rect.width(), row_h));
            p.rect_filled(row, 2.0, track);
            let w = rect.width() * meter_fraction(rms);
            if w > 0.0 {
                let bar = Rect::from_min_size(row.min, vec2(w, row_h));
                p.rect_filled(bar, 2.0, color.gamma_multiply(0.85));
            }
            let Some(peak) = peak_db.map(|pk| pk[i]) else {
                continue;
            };
            if peak.is_finite() && peak > METER_FLOOR_DB {
                // Near full scale the tick gets wider instead of red (red is the right ear).
                let over = peak > -1.0;
                let tw = if over { 4.0 } else { 2.0 };
                let px = (rect.width() - tw / 2.0).min(rect.width() * meter_fraction(peak));
                let tick = Rect::from_min_size(
                    pos2(rect.left() + px - tw / 2.0, y - 1.0),
                    vec2(tw, row_h + 2.0),
                );
                p.rect_filled(tick, 0.0, fg.gamma_multiply(if over { 1.0 } else { 0.8 }));
            }
        }
    });
}

fn meter_fraction(db: f32) -> f32 {
    if !db.is_finite() {
        return 0.0;
    }
    ((db - METER_FLOOR_DB) / -METER_FLOOR_DB).clamp(0.0, 1.0)
}

// MARK: Shared plot painting

/// Font of the axis ticks.
pub(crate) fn tick_font() -> FontId {
    FontId::proportional(12.0)
}

/// The text colour at an opacity (SwiftUI `Color.primary.opacity(a)`).
pub(crate) fn primary(v: &Visuals, a: f32) -> Color32 {
    v.strong_text_color().gamma_multiply(a)
}

/// Secondary text (SwiftUI `.secondary`).
pub(crate) fn secondary(v: &Visuals) -> Color32 {
    v.weak_text_color()
}

/// The card behind a chart: rounded 10, text background, a faint border.
pub(crate) fn paint_card(p: &Painter, rect: Rect, v: &Visuals) {
    p.rect(
        rect,
        10.0,
        v.extreme_bg_color,
        Stroke::new(1.0, primary(v, 0.12)),
        StrokeKind::Inside,
    );
}

pub(crate) fn hline(p: &Painter, x0: f32, x1: f32, y: f32, stroke: Stroke) {
    p.line_segment([pos2(x0, y), pos2(x1, y)], stroke);
}

pub(crate) fn dashed_hline(
    p: &Painter,
    x0: f32,
    x1: f32,
    y: f32,
    stroke: Stroke,
    dash: f32,
    gap: f32,
) {
    p.extend(Shape::dashed_line(
        &[pos2(x0, y), pos2(x1, y)],
        stroke,
        dash,
        gap,
    ));
}

/// A small rounded label with an ear-coloured outline above `anchor` (below it when there is
/// no room), kept inside `bounds` (Swift `drawPill`).
pub(crate) fn paint_pill(
    p: &Painter,
    v: &Visuals,
    text: &str,
    anchor: Pos2,
    gap: f32,
    bounds: Rect,
    color: Color32,
) {
    let galley = p.layout_no_wrap(text.to_owned(), FontId::proportional(13.0), v.text_color());
    let w = galley.size().x + 14.0;
    let h = galley.size().y + 6.0;
    let mut y = anchor.y - gap - h;
    if y < bounds.top() {
        y = anchor.y + gap;
    }
    let x = (anchor.x - w / 2.0)
        .max(bounds.left())
        .min(bounds.right() - w);
    let rect = Rect::from_min_size(pos2(x, y), vec2(w, h));
    p.rect(
        rect,
        h / 2.0,
        v.panel_fill,
        Stroke::new(1.0, color),
        StrokeKind::Inside,
    );
    p.galley(rect.center() - galley.size() / 2.0, galley, v.text_color());
}
