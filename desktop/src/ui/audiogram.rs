//! The audiogram editor (Swift `AudiogramEditorView`): normal-hearing band, grid, both ears'
//! curves and markers, and the selected point's ring and label. Pointer and keys go to the
//! `Editor` input methods.

use std::ops::RangeInclusive;
use std::time::Instant;

use audiocurve_core::profile::{Audiogram, Ear};
use egui::{
    Align2, EventFilter, Pos2, Rect, Response, Role, Sense, Shape, Stroke, Ui, WidgetInfo, pos2,
    vec2,
};

use crate::i18n::tr;
use crate::model::editing_input::{Key, fmt};
use crate::model::editor::Editor;
use crate::ui::geometry::PlotGeometry;
use crate::ui::theme;
use crate::ui::widgets::{hline, paint_card, paint_pill, primary, secondary, tick_font};

pub fn show(ui: &mut Ui, editor: &mut Editor, now: Instant) {
    let size = ui.available_size().max(vec2(0.0, 0.0));
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click_and_drag());
    handle_input(ui, &resp, editor, 0.0..=40.0, true, now);
    let value = editor
        .selection
        .map(|s| editor.point_label(s))
        .filter(|s| !s.is_empty());
    resp.widget_info(|| WidgetInfo {
        current_text_value: value.clone(),
        ..WidgetInfo::labeled(Role::Canvas, true, tr("Audiogram"))
    });
    if ui.is_rect_visible(rect) {
        draw(ui, rect, editor);
    }
}

/// A read-only audiogram of both ears at full strength (Swift `AudiogramChart`), for the
/// hearing test result.
pub fn show_read_only(ui: &mut Ui, left: &Audiogram, right: &Audiogram, height: f32) {
    let size = vec2(ui.available_width().max(0.0), height);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::hover());
    resp.widget_info(|| WidgetInfo::labeled(Role::Image, true, tr("Audiogram")));
    if !ui.is_rect_visible(rect) {
        return;
    }
    let p = ui.painter_at(rect);
    paint_card(&p, rect, ui.visuals());
    let g = paint_grid(&p, ui.visuals(), rect);
    for (ear, a) in [(Ear::Left, left), (Ear::Right, right)] {
        paint_curve(&p, rect, &g, ear, a, 0.7, 1.0);
    }
}

/// Screen position of a point in the plot of `g` placed at `rect`.
fn plot_pos(rect: Rect, g: &PlotGeometry, f: f64, db: f64) -> Pos2 {
    pos2(rect.left() + g.x(f) as f32, rect.top() + g.y(db) as f32)
}

/// The plot area of `g` placed at `rect`.
fn plot_rect(rect: Rect, g: &PlotGeometry) -> Rect {
    Rect::from_min_max(
        pos2(
            rect.left() + g.plot.min_x as f32,
            rect.top() + g.plot.min_y as f32,
        ),
        pos2(
            rect.left() + g.plot.max_x() as f32,
            rect.top() + g.plot.max_y() as f32,
        ),
    )
}

/// The normal-hearing band, the grid and the axis labels. Returns the plot geometry.
fn paint_grid(p: &egui::Painter, v: &egui::Visuals, rect: Rect) -> PlotGeometry {
    let g = PlotGeometry::new(rect.width() as f64, rect.height() as f64);
    let at = |f: f64, db: f64| plot_pos(rect, &g, f, db);
    let r = plot_rect(rect, &g);
    let (left, right, top, bottom) = (r.left(), r.right(), r.top(), r.bottom());
    let grid = primary(v, 0.08);
    let tick = secondary(v);

    // Normal-hearing band (−10…20 dB HL).
    p.rect_filled(
        Rect::from_x_y_ranges(left..=right, at(0.0, -10.0).y..=at(0.0, 20.0).y),
        0.0,
        theme::NORMAL_BAND,
    );

    // Horizontal grid every 10 dB, labels every 20.
    for db in (-10..=120).step_by(10) {
        let y = at(125.0, db as f64).y;
        let color = if db == 0 { primary(v, 0.22) } else { grid };
        hline(p, left, right, y, Stroke::new(1.0, color));
        if db >= 0 && db % 20 == 0 {
            p.text(
                pos2(left - 8.0, y),
                Align2::RIGHT_CENTER,
                fmt::db(db as f64),
                tick_font(),
                tick,
            );
        }
    }

    // Vertical grid at the standard frequencies.
    for f in Audiogram::STANDARD_FREQUENCIES {
        let x = at(f, 0.0).x;
        p.line_segment([pos2(x, top), pos2(x, bottom)], Stroke::new(1.0, grid));
        p.text(
            pos2(x, bottom + 8.0),
            Align2::CENTER_TOP,
            fmt::axis_freq(f),
            tick_font(),
            tick,
        );
    }
    g
}

/// One ear's curve and markers, the line and markers at the given opacities.
fn paint_curve(
    p: &egui::Painter,
    rect: Rect,
    g: &PlotGeometry,
    ear: Ear,
    a: &Audiogram,
    line_alpha: f32,
    marker_alpha: f32,
) {
    let color = theme::ear_color(ear);
    let pts: Vec<Pos2> = a
        .points
        .iter()
        .map(|q| plot_pos(rect, g, q.frequency_hz, q.threshold_db_hl))
        .collect();
    p.add(Shape::line(
        pts.clone(),
        Stroke::new(1.5, color.gamma_multiply(line_alpha)),
    ));
    for c in pts {
        draw_marker(p, ear, c, color.gamma_multiply(marker_alpha));
    }
}

fn draw(ui: &Ui, rect: Rect, editor: &Editor) {
    let p = ui.painter_at(rect);
    let v = ui.visuals();
    paint_card(&p, rect, v);
    let Some(profile) = &editor.profile else {
        return;
    };
    let g = paint_grid(&p, v, rect);
    let at = |f: f64, db: f64| plot_pos(rect, &g, f, db);

    // Curves and markers: inactive ear first so the active one is on top.
    for ear in [editor.active_ear.other(), editor.active_ear] {
        let active = ear == editor.active_ear;
        let (line, marker) = if active { (0.7, 1.0) } else { (0.4, 0.75) };
        paint_curve(&p, rect, &g, ear, profile.audiogram(ear), line, marker);
    }

    // Selected point: focus ring and label.
    if let Some(s) = editor.selection
        && let Some(pt) = profile
            .audiogram(s.ear)
            .points
            .iter()
            .find(|q| q.frequency_hz == s.frequency_hz)
    {
        let c = at(pt.frequency_hz, pt.threshold_db_hl);
        p.circle_stroke(c, 11.0, Stroke::new(2.0, v.selection.bg_fill));
        let bounds = plot_rect(rect, &g);
        paint_pill(
            &p,
            v,
            &editor.point_label(s),
            c,
            16.0,
            bounds,
            theme::ear_color(s.ear),
        );
    }
}

/// Left ear: a cross; right ear: a ring.
fn draw_marker(p: &egui::Painter, ear: Ear, c: Pos2, color: egui::Color32) {
    let r = 5.5;
    let s = Stroke::new(2.0, color);
    match ear {
        Ear::Left => {
            p.line_segment([c - vec2(r, r), c + vec2(r, r)], s);
            p.line_segment([c + vec2(-r, r), c + vec2(r, -r)], s);
        }
        Ear::Right => {
            p.circle_stroke(c, r, s);
        }
    }
}

/// Pointer and keyboard input of an editable plot (the audiogram and the gain chart): a press
/// focuses the plot and begins a drag at the press position, moves drag (with Option as `fine`
/// when `fine_drag`), release ends it, a double click resets a trim, and key presses go to
/// `Editor::handle_key` while the plot has focus. Presses with Cmd or Ctrl held are ignored.
pub(crate) fn handle_input(
    ui: &Ui,
    resp: &Response,
    editor: &mut Editor,
    gain_range: RangeInclusive<f64>,
    fine_drag: bool,
    now: Instant,
) {
    let rect = resp.rect;
    let (w, h) = (rect.width() as f64, rect.height() as f64);
    let local = |p: Pos2| ((p.x - rect.left()) as f64, (p.y - rect.top()) as f64);
    let down_id = resp.id.with("pointer_down");
    let was_down = ui.data(|d| d.get_temp::<bool>(down_id)).unwrap_or(false);
    let down = resp.is_pointer_button_down_on();
    let (press, latest, alt) = ui.input(|i| {
        (
            i.pointer.press_origin(),
            i.pointer.latest_pos(),
            i.modifiers.alt,
        )
    });
    if down && !was_down {
        resp.request_focus();
        if let Some(p) = press {
            let (x, y) = local(p);
            editor.begin_drag(x, y, w, h, gain_range.clone(), now);
        }
    }
    if down {
        if let Some(p) = latest {
            let (x, y) = local(p);
            editor.drag(x, y, w, h, fine_drag && alt, now);
        }
    } else if was_down {
        editor.end_drag();
    }
    if down != was_down {
        ui.data_mut(|d| d.insert_temp(down_id, down));
    }
    if resp.double_clicked()
        && let Some(p) = resp.interact_pointer_pos()
    {
        let (x, y) = local(p);
        editor.reset_trim_at(x, y, w, h, gain_range, now);
    }
    if resp.has_focus() {
        ui.memory_mut(|m| {
            m.set_focus_lock_filter(
                resp.id,
                EventFilter {
                    tab: true,
                    horizontal_arrows: true,
                    vertical_arrows: true,
                    escape: true,
                },
            )
        });
        let keys: Vec<(Key, bool)> = ui.input(|i| {
            i.events
                .iter()
                .filter_map(|e| match e {
                    egui::Event::Key {
                        key,
                        pressed: true,
                        modifiers,
                        ..
                    } if !(modifiers.command || modifiers.mac_cmd || modifiers.ctrl) => {
                        editor_key(*key).map(|k| (k, modifiers.alt))
                    }
                    _ => None,
                })
                .collect()
        });
        for (k, fine) in keys {
            editor.handle_key(k, fine, now);
        }
    }
}

fn editor_key(key: egui::Key) -> Option<Key> {
    use egui::Key as K;
    Some(match key {
        K::ArrowUp => Key::Up,
        K::ArrowDown => Key::Down,
        K::ArrowLeft => Key::Left,
        K::ArrowRight => Key::Right,
        K::Tab => Key::Tab,
        K::Backspace | K::Delete => Key::Delete,
        K::Enter => Key::Return,
        K::Escape => Key::Escape,
        K::Minus => Key::Char('-'),
        K::Num0 => Key::Char('0'),
        K::Num1 => Key::Char('1'),
        K::Num2 => Key::Char('2'),
        K::Num3 => Key::Char('3'),
        K::Num4 => Key::Char('4'),
        K::Num5 => Key::Char('5'),
        K::Num6 => Key::Char('6'),
        K::Num7 => Key::Char('7'),
        K::Num8 => Key::Char('8'),
        K::Num9 => Key::Char('9'),
        _ => return None,
    })
}
