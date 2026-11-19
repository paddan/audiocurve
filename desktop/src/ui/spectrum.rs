//! The spectrum (Swift `SpectrumView`): input as a soft grey area, output per ear as a
//! coloured line over a faint fill, the output ceiling (MPO) as a dashed red line with the zone
//! above it tinted, and an L / R / Both picker in the top-left corner.

use std::time::Duration;

use audiocurve_core::profile::Ear;
use audiocurve_dsp::Spectrum;
use egui::{
    Align2, Color32, Mesh, Pos2, Rect, Role, Sense, Shape, Stroke, Ui, UiBuilder, WidgetInfo, pos2,
    vec2,
};

use crate::i18n::tr;
use crate::model::app::AppModel;
use crate::model::editing_input::fmt;
use crate::ui::theme;
use crate::ui::widgets::{
    dashed_hline, hline, paint_card, primary, secondary, segmented, tick_font,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SpectrumEars {
    Left,
    Right,
    #[default]
    Both,
}

impl SpectrumEars {
    fn shown(self) -> &'static [Ear] {
        match self {
            SpectrumEars::Left => &[Ear::Left],
            SpectrumEars::Right => &[Ear::Right],
            SpectrumEars::Both => &[Ear::Left, Ear::Right],
        }
    }
}

const LEFT_INSET: f32 = 40.0;
const RIGHT_INSET: f32 = 20.0;
const TOP_INSET: f32 = 16.0;
const BOTTOM_INSET: f32 = 28.0;
const F_MIN: f64 = 20.0;
const F_MAX: f64 = 20_000.0;
const DB_MIN: f64 = -100.0;
const DB_MAX: f64 = 0.0;
const TICKS: [f64; 9] = [
    50.0, 100.0, 200.0, 500.0, 1000.0, 2000.0, 5000.0, 10_000.0, 20_000.0,
];
/// The chart redraws 20 times a second.
const REFRESH: Duration = Duration::from_millis(50);

/// Frequency (log, 20 Hz…20 kHz) and level (−100…0 dBFS) to screen positions.
struct Geometry {
    plot: Rect,
}

impl Geometry {
    fn new(rect: Rect) -> Geometry {
        let min = rect.min + vec2(LEFT_INSET, TOP_INSET);
        let size = vec2(
            (rect.width() - LEFT_INSET - RIGHT_INSET).max(1.0),
            (rect.height() - TOP_INSET - BOTTOM_INSET).max(1.0),
        );
        Geometry {
            plot: Rect::from_min_size(min, size),
        }
    }

    fn x(&self, hz: f64) -> f32 {
        let t = (hz / F_MIN).log10() / (F_MAX / F_MIN).log10();
        self.plot.left() + t as f32 * self.plot.width()
    }

    fn y(&self, db: f64) -> f32 {
        let c = db.clamp(DB_MIN, DB_MAX);
        let c = if c.is_nan() { DB_MIN } else { c };
        self.plot.bottom() - ((c - DB_MIN) / (DB_MAX - DB_MIN)) as f32 * self.plot.height()
    }
}

pub fn show(ui: &mut Ui, m: &AppModel, ears: &mut SpectrumEars) {
    let size = ui.available_size().max(vec2(0.0, 0.0));
    let (rect, resp) = ui.allocate_exact_size(size, Sense::hover());
    let data = m.engine().spectrum();
    let has_data = data.freq_hz.iter().any(|f| *f > 0.0);
    let data = has_data.then_some(&data);
    let description = describe(data, *ears);
    resp.widget_info(|| WidgetInfo {
        current_text_value: Some(description.clone()),
        ..WidgetInfo::labeled(Role::Image, true, tr("Spectrum"))
    });
    if ui.is_rect_visible(rect) {
        draw(ui, rect, data, *ears, m.levels.mpo_dbfs);
    }
    // The picker sits on the chart, without taking space in the parent.
    let picker = Rect::from_min_size(
        rect.min + vec2(LEFT_INSET + 8.0, TOP_INSET + 6.0),
        vec2(f32::INFINITY, ui.spacing().interact_size.y),
    );
    let mut child = ui.new_child(UiBuilder::new().max_rect(picker));
    let (l, r) = (fmt::ear_initial(Ear::Left), fmt::ear_initial(Ear::Right));
    let both = tr("Both");
    segmented(
        &mut child,
        ears,
        &[
            (SpectrumEars::Left, l.as_str()),
            (SpectrumEars::Right, r.as_str()),
            (SpectrumEars::Both, both.as_str()),
        ],
    );
    ui.ctx().request_repaint_after(REFRESH);
}

fn draw(ui: &Ui, rect: Rect, data: Option<&Spectrum>, ears: SpectrumEars, mpo: f64) {
    let p = ui.painter_at(rect);
    let v = ui.visuals();
    paint_card(&p, rect, v);
    let g = Geometry::new(rect);
    let plot_painter = p.with_clip_rect(g.plot.intersect(rect));

    // Plot background and the zone above the ceiling.
    p.rect_filled(g.plot, 6.0, primary(v, 0.03));
    let ty = g.y(mpo);
    plot_painter.rect_filled(
        Rect::from_x_y_ranges(g.plot.x_range(), g.plot.top()..=ty.max(g.plot.top())),
        6.0,
        theme::ALERT.gamma_multiply(0.06),
    );

    let grid = primary(v, 0.06);
    let tick = secondary(v);
    for i in 0..=10 {
        let db = DB_MIN + i as f64 * 10.0;
        let major = i % 2 == 0;
        let w = if major { 1.0 } else { 0.5 };
        hline(
            &p,
            g.plot.left(),
            g.plot.right(),
            g.y(db),
            Stroke::new(w, grid),
        );
        if major {
            p.text(
                pos2(g.plot.left() - 8.0, g.y(db)),
                Align2::RIGHT_CENTER,
                fmt::db(db),
                tick_font(),
                tick,
            );
        }
    }
    for f in TICKS {
        let x = g.x(f);
        p.line_segment(
            [pos2(x, g.plot.top()), pos2(x, g.plot.bottom())],
            Stroke::new(0.5, grid),
        );
        p.text(
            pos2(x, g.plot.bottom() + 8.0),
            Align2::CENTER_TOP,
            fmt::axis_freq(f),
            tick_font(),
            tick,
        );
    }

    if let Some(data) = data {
        let input: Vec<f64> = match ears {
            SpectrumEars::Left => row(&data.in_db[0]),
            SpectrumEars::Right => row(&data.in_db[1]),
            SpectrumEars::Both => input_both(data),
        };
        let grey = secondary(v);
        let input_curve = curve(&g, &data.freq_hz, &input);
        plot_painter.add(area(
            &g,
            &input_curve,
            grey.gamma_multiply(0.28),
            grey.gamma_multiply(0.08),
        ));
        let fill = if ears == SpectrumEars::Both {
            0.12
        } else {
            0.2
        };
        let outputs: Vec<(Ear, Vec<Pos2>)> = ears
            .shown()
            .iter()
            .map(|&ear| {
                (
                    ear,
                    curve(&g, &data.freq_hz, &row(&data.out_db[ear_index(ear)])),
                )
            })
            .collect();
        for (ear, c) in &outputs {
            let color = theme::ear_color(*ear);
            plot_painter.add(area(
                &g,
                c,
                color.gamma_multiply(fill),
                Color32::TRANSPARENT,
            ));
        }
        for (ear, c) in outputs {
            plot_painter.add(Shape::line(c, Stroke::new(2.0, theme::ear_color(ear))));
        }
    }

    // Output ceiling (MPO).
    dashed_hline(
        &p,
        g.plot.left(),
        g.plot.right(),
        ty,
        Stroke::new(1.0, theme::ALERT.gamma_multiply(0.7)),
        4.0,
        3.0,
    );
    p.text(
        pos2(g.plot.right() - 6.0, ty + 3.0),
        Align2::RIGHT_TOP,
        tr("Ceiling"),
        tick_font(),
        theme::ALERT.gamma_multiply(0.8),
    );
}

fn ear_index(ear: Ear) -> usize {
    match ear {
        Ear::Left => 0,
        Ear::Right => 1,
    }
}

fn row(r: &[f32]) -> Vec<f64> {
    r.iter().map(|&v| v as f64).collect()
}

/// Power average of both ears' input.
fn input_both(s: &Spectrum) -> Vec<f64> {
    s.in_db[0]
        .iter()
        .zip(&s.in_db[1])
        .map(|(&a, &b)| {
            let p = (10f64.powf(a as f64 / 10.0) + 10f64.powf(b as f64 / 10.0)) / 2.0;
            10.0 * p.log10()
        })
        .collect()
}

/// Smooth curve through the points (Catmull-Rom as cubic Béziers), flattened. Points at a
/// frequency of 0 or below are left out.
fn curve(g: &Geometry, freq: &[f32], db: &[f64]) -> Vec<Pos2> {
    let pts: Vec<Pos2> = freq
        .iter()
        .zip(db)
        .filter(|(f, _)| **f > 0.0)
        .map(|(&f, &d)| pos2(g.x(f as f64), g.y(d)))
        .filter(|p| p.x.is_finite() && p.y.is_finite())
        .collect();
    let Some(&first) = pts.first() else {
        return pts;
    };
    const STEPS: usize = 6;
    let mut out = Vec::with_capacity(pts.len() * STEPS);
    out.push(first);
    for i in 1..pts.len() {
        let p0 = pts[i.saturating_sub(2)];
        let p1 = pts[i - 1];
        let p2 = pts[i];
        let p3 = pts[(i + 1).min(pts.len() - 1)];
        let c1 = p1 + (p2 - p0) / 6.0;
        let c2 = p2 - (p3 - p1) / 6.0;
        for s in 1..=STEPS {
            let t = s as f32 / STEPS as f32;
            let u = 1.0 - t;
            let q = p1.to_vec2() * (u * u * u)
                + c1.to_vec2() * (3.0 * u * u * t)
                + c2.to_vec2() * (3.0 * u * t * t)
                + p2.to_vec2() * (t * t * t);
            out.push(q.to_pos2());
        }
    }
    out
}

/// The area under a curve down to the plot's bottom, with a vertical gradient from `top` (at
/// the plot's top) to `bottom` (at its bottom).
fn area(g: &Geometry, curve: &[Pos2], top: Color32, bottom: Color32) -> Shape {
    let shade = |y: f32| {
        let t = ((y - g.plot.top()) / g.plot.height()).clamp(0.0, 1.0);
        top.lerp_to_gamma(bottom, t)
    };
    let base = g.plot.bottom();
    let mut mesh = Mesh::default();
    for w in curve.windows(2) {
        let (a, b) = (w[0], w[1]);
        let i = mesh.vertices.len() as u32;
        mesh.colored_vertex(a, shade(a.y));
        mesh.colored_vertex(b, shade(b.y));
        mesh.colored_vertex(pos2(b.x, base), bottom);
        mesh.colored_vertex(pos2(a.x, base), bottom);
        mesh.add_triangle(i, i + 1, i + 2);
        mesh.add_triangle(i, i + 2, i + 3);
    }
    Shape::mesh(mesh)
}

/// For VoiceOver: the output level near 125 Hz, 1 kHz and 4 kHz per shown ear.
fn describe(data: Option<&Spectrum>, ears: SpectrumEars) -> String {
    let Some(data) = data else {
        return tr("No signal");
    };
    ears.shown()
        .iter()
        .map(|&ear| {
            let r = &data.out_db[ear_index(ear)];
            let parts: Vec<String> = [125.0, 1000.0, 4000.0]
                .iter()
                .map(|&hz| {
                    format!(
                        "{} {} dBFS",
                        fmt::freq(hz),
                        fmt::db(level_near(data, r, hz))
                    )
                })
                .collect();
            format!("{}: {}", fmt::ear_name(ear), parts.join(", "))
        })
        .collect::<Vec<_>>()
        .join(". ")
}

/// Level of the point nearest `hz` on a log axis.
fn level_near(data: &Spectrum, r: &[f32], hz: f64) -> f64 {
    let dist = |f: f32| {
        if f <= 0.0 {
            f64::INFINITY
        } else {
            (f as f64 / hz).ln().abs()
        }
    };
    data.freq_hz
        .iter()
        .zip(r)
        .min_by(|a, b| dist(*a.0).total_cmp(&dist(*b.0)))
        .map_or(-140.0, |(_, &v)| v as f64)
}
