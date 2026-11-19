//! The gain chart (Swift `GainView`): per band and ear a bar of the soft-input gain of the
//! fitting in effect, a dashed marker for the loud-input gain, and ticks for the speech (solid)
//! and music (dotted) prescriptions. Dragging a bar trims it; a double click resets the trim.

use std::ops::RangeInclusive;
use std::time::Instant;

use audiocurve_core::fitting::{self, CompressionMode, Prescription};
use audiocurve_core::profile::{Ear, HearingProfile};
use audiocurve_core::{BAND_COUNT, CENTERS};
use audiocurve_dsp::BandFitting;
use egui::{Align2, Rect, Role, Sense, Stroke, StrokeKind, Ui, WidgetInfo, pos2, vec2};

use crate::i18n::tr;
use crate::model::app::AppModel;
use crate::model::editing_input::{DragTarget, fmt};
use crate::model::levels::Levels;
use crate::ui::audiogram::handle_input;
use crate::ui::geometry::GainGeometry;
use crate::ui::theme;
use crate::ui::widgets::{
    dashed_hline, hline, paint_card, paint_pill, primary, secondary, tick_font,
};

type Bands = [BandFitting; BAND_COUNT];

/// Both prescriptions for one ear, from the same profile and levels.
fn fittings(p: &HearingProfile, ear: Ear, l: &Levels) -> (Bands, Bands) {
    let (s, t) = (l.strength / 100.0, l.tone / 100.0);
    (
        fitting::bands(p, ear, s, t, Prescription::Speech),
        fitting::bands(p, ear, s, t, Prescription::Music),
    )
}

/// The fitting in effect: the chosen prescription, or in Auto the classifier-weighted blend.
fn in_effect(both: &(Bands, Bands), mode: CompressionMode, speech: f64) -> Bands {
    match mode {
        CompressionMode::Speech => both.0,
        CompressionMode::Music => both.1,
        CompressionMode::Auto => fitting::blend(&both.0, &both.1, speech),
    }
}

/// Axis range covering both prescriptions of both ears, so it does not move with the classifier.
fn gain_range(p: &HearingProfile, l: &Levels) -> RangeInclusive<f64> {
    let (mut lo, mut hi) = (0.0_f64, 40.0_f64);
    for ear in [Ear::Left, Ear::Right] {
        let (s, m) = fittings(p, ear, l);
        for b in s.iter().chain(m.iter()) {
            hi = hi.max(b.gain_soft_db).max(b.gain_loud_db);
            lo = lo.min(b.gain_soft_db).min(b.gain_loud_db);
        }
    }
    (((lo - 5.0) / 10.0).floor() * 10.0)..=(((hi + 5.0) / 10.0).ceil() * 10.0)
}

pub fn show(ui: &mut Ui, m: &mut AppModel, now: Instant) {
    let size = ui.available_size().max(vec2(0.0, 0.0));
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click_and_drag());
    let speech = m.engine().meters().speech as f64;
    let range = current_range(m);
    handle_input(ui, &resp, &mut m.editor, range, false, now);

    let range = current_range(m);
    let levels = m.levels;
    let editor = &m.editor;
    let fits = editor.profile.as_ref().map(|p| {
        [Ear::Left, Ear::Right].map(|ear| {
            let both = fittings(p, ear, &levels);
            let effect = in_effect(&both, levels.mode, speech);
            (both, effect)
        })
    });
    let label = editor.trim_selection.and_then(|t| {
        let f = fits.as_ref()?;
        Some(editor.trim_label(t, f[ear_index(t.ear)].1[t.band].gain_soft_db))
    });
    resp.widget_info(|| WidgetInfo {
        current_text_value: label.clone(),
        ..WidgetInfo::labeled(Role::Canvas, true, tr("Gain per band"))
    });
    if !ui.is_rect_visible(rect) {
        return;
    }
    let p = ui.painter_at(rect);
    let v = ui.visuals();
    paint_card(&p, rect, v);
    let (Some(profile), Some(fits)) = (&editor.profile, fits) else {
        return;
    };
    let g = GainGeometry::new(rect.width() as f64, rect.height() as f64, range.clone());
    let y = |db: f64| rect.top() + g.y(db) as f32;
    let (left, right) = (
        rect.left() + g.plot.min_x as f32,
        rect.left() + g.plot.max_x() as f32,
    );
    let bottom = rect.top() + g.plot.max_y() as f32;
    let grid = primary(v, 0.08);
    let tick = secondary(v);

    let lines = ((range.end() - range.start()) / 10.0)
        .round()
        .clamp(0.0, 100.0) as usize;
    for i in 0..=lines {
        let db = range.start() + i as f64 * 10.0;
        let color = if db == 0.0 { primary(v, 0.3) } else { grid };
        hline(&p, left, right, y(db), Stroke::new(1.0, color));
        p.text(
            pos2(left - 8.0, y(db)),
            Align2::RIGHT_CENTER,
            fmt::db(db),
            tick_font(),
            tick,
        );
    }
    for (band, f) in CENTERS.iter().enumerate() {
        p.text(
            pos2(rect.left() + g.x(band) as f32, bottom + 8.0),
            Align2::CENTER_TOP,
            fmt::axis_freq(*f),
            tick_font(),
            tick,
        );
    }

    let bar = |ear: Ear, band: usize, value: f64| {
        let r = bar_rect(&g, ear, band, value);
        r.translate(rect.min.to_vec2())
    };
    for ear in [Ear::Left, Ear::Right] {
        let color = theme::ear_color(ear);
        let ((speech_rx, music_rx), effect) = &fits[ear_index(ear)];
        let trims = profile.trim(ear);
        for band in 0..BAND_COUNT {
            let r = bar(ear, band, effect[band].gain_soft_db);
            let trimmed = trims.get(band).is_some_and(|t| *t != 0.0);
            p.rect_filled(
                r,
                2.0,
                color.gamma_multiply(if trimmed { 0.85 } else { 0.6 }),
            );
            // Gain for loud input: dashed marker across the bar.
            dashed_hline(
                &p,
                r.left() - 2.0,
                r.right() + 2.0,
                y(effect[band].gain_loud_db),
                Stroke::new(1.5, primary(v, 0.85)),
                3.0,
                2.0,
            );
            // Soft gain of each prescription: speech solid, music dotted.
            let s = Stroke::new(1.5, tick);
            hline(
                &p,
                r.left() - 3.0,
                r.right() + 3.0,
                y(speech_rx[band].gain_soft_db),
                s,
            );
            dashed_hline(
                &p,
                r.left() - 3.0,
                r.right() + 3.0,
                y(music_rx[band].gain_soft_db),
                s,
                1.0,
                2.0,
            );
        }
    }

    if let Some(t) = editor.trim_selection.filter(|t| t.band < BAND_COUNT) {
        let soft = fits[ear_index(t.ear)].1[t.band].gain_soft_db;
        let r = bar(t.ear, t.band, soft);
        p.rect_stroke(
            r.expand(3.0),
            4.0,
            Stroke::new(2.0, v.selection.bg_fill),
            StrokeKind::Middle,
        );
        let bounds =
            Rect::from_x_y_ranges(left..=right, (rect.top() + g.plot.min_y as f32)..=bottom);
        paint_pill(
            &p,
            v,
            &label.unwrap_or_default(),
            pos2(r.center().x, r.top()),
            10.0,
            bounds,
            theme::ear_color(t.ear),
        );
    }
}

/// The range frozen by a running trim drag, otherwise the one the levels give.
fn current_range(m: &AppModel) -> RangeInclusive<f64> {
    if let Some(DragTarget::Trim { range, .. }) = m.editor.drag.as_ref().map(|d| &d.target) {
        return range.clone();
    }
    match &m.editor.profile {
        Some(p) => gain_range(p, &m.levels),
        None => 0.0..=40.0,
    }
}

fn ear_index(ear: Ear) -> usize {
    match ear {
        Ear::Left => 0,
        Ear::Right => 1,
    }
}

/// A bar from 0 dB to `value`, left of the slot centre for the left ear, right of it for the right.
fn bar_rect(g: &GainGeometry, ear: Ear, band: usize, value: f64) -> Rect {
    let center = g.x(band) as f32;
    let bw = g.bar_width() as f32;
    let x = match ear {
        Ear::Left => center - bw - 1.0,
        Ear::Right => center + 1.0,
    };
    let (y0, y1) = (g.y(0.0) as f32, g.y(value) as f32);
    Rect::from_min_size(pos2(x, y0.min(y1)), vec2(bw, (y1 - y0).abs().max(1.0)))
}
