//! Pure editing operations on a `HearingProfile` for click, drag and keyboard editing of the
//! audiogram (Swift `AudiogramEditing`).

use crate::profile::{Audiogram, AudiogramPoint, Ear, HearingProfile};

/// A single audiogram point, addressed by ear and frequency.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PointSelection {
    pub ear: Ear,
    pub frequency_hz: f64,
}

pub const MIN_POINTS: usize = 4;

/// Rounds to the nearest 5 dB (or 1 dB when fine), then clamps to −10…120.
pub fn snap(db_hl: f64, fine: bool) -> f64 {
    let step = if fine { 1.0 } else { 5.0 };
    ((db_hl / step).round() * step).clamp(-10.0, 120.0)
}

fn index_of(a: &Audiogram, f: f64) -> Option<usize> {
    a.points.iter().position(|p| p.frequency_hz == f)
}

/// Sets the threshold at the selected frequency. Adds the point if missing, but only for
/// standard frequencies.
pub fn set_value(p: &mut HearingProfile, s: PointSelection, db_hl: f64, fine: bool) {
    let value = snap(db_hl, fine);
    let mut a = p.audiogram(s.ear).clone();
    if let Some(i) = index_of(&a, s.frequency_hz) {
        a.points[i].threshold_db_hl = value;
    } else if Audiogram::STANDARD_FREQUENCIES.contains(&s.frequency_hz) {
        a.points.push(AudiogramPoint { frequency_hz: s.frequency_hz, threshold_db_hl: value });
        a.points.sort_by(|x, y| x.frequency_hz.total_cmp(&y.frequency_hz));
    } else {
        return;
    }
    p.set_audiogram(s.ear, a);
}

/// Moves the threshold one step: `up` lowers dB HL (−5, or −1 when fine), otherwise raises it.
/// No-op if the point is missing.
pub fn nudge(p: &mut HearingProfile, s: PointSelection, up: bool, fine: bool) {
    let mut a = p.audiogram(s.ear).clone();
    let Some(i) = index_of(&a, s.frequency_hz) else { return };
    let step = if fine { 1.0 } else { 5.0 };
    let delta = if up { -step } else { step };
    a.points[i].threshold_db_hl = snap(a.points[i].threshold_db_hl + delta, fine);
    p.set_audiogram(s.ear, a);
}

/// The next (or previous) existing point on the same ear, skipping missing frequencies.
pub fn neighbor(p: &HearingProfile, s: PointSelection, forward: bool) -> Option<PointSelection> {
    let points = &p.audiogram(s.ear).points;
    let target = if forward {
        points.iter().find(|q| q.frequency_hz > s.frequency_hz)
    } else {
        points.iter().rev().find(|q| q.frequency_hz < s.frequency_hz)
    };
    target.map(|q| PointSelection { ear: s.ear, frequency_hz: q.frequency_hz })
}

/// Same frequency on the other ear if present, otherwise the nearest one in log-frequency
/// (ties go to the higher). If the other ear has no points, the selection is returned unchanged.
pub fn switch_ear(p: &HearingProfile, s: PointSelection) -> PointSelection {
    let other = s.ear.other();
    let points = &p.audiogram(other).points;
    let Some(first) = points.first() else { return s };
    if points.iter().any(|q| q.frequency_hz == s.frequency_hz) {
        return PointSelection { ear: other, frequency_hz: s.frequency_hz };
    }
    let target = s.frequency_hz.log2();
    let mut best = *first;
    let mut best_distance = (best.frequency_hz.log2() - target).abs();
    for candidate in &points[1..] {
        let distance = (candidate.frequency_hz.log2() - target).abs();
        if distance < best_distance - 1e-12
            || ((distance - best_distance).abs() <= 1e-12 && candidate.frequency_hz > best.frequency_hz)
        {
            best = *candidate;
            best_distance = distance;
        }
    }
    PointSelection { ear: other, frequency_hz: best.frequency_hz }
}

/// Removes the selected point. Returns false (and leaves the profile unchanged) if the point is
/// missing or if fewer than `MIN_POINTS` would remain.
pub fn delete(p: &mut HearingProfile, s: PointSelection) -> bool {
    let mut a = p.audiogram(s.ear).clone();
    if a.points.len() <= MIN_POINTS {
        return false;
    }
    let Some(i) = index_of(&a, s.frequency_hz) else { return false };
    a.points.remove(i);
    p.set_audiogram(s.ear, a);
    true
}

/// Parses a typed dB HL value. Accepts ASCII "-" and Unicode minus "−"; None outside −10…120.
pub fn parse_typed(text: &str) -> Option<f64> {
    let normalized = text.trim().replace('\u{2212}', "-");
    let value: f64 = normalized.parse().ok()?;
    (-10.0..=120.0).contains(&value).then_some(value)
}

/// Maps a vertical position in the plot to dB HL: y = 0 is −10, y = plot_height is 120. Not rounded.
pub fn db_hl_at_y(y: f64, plot_height: f64) -> f64 {
    if plot_height > 0.0 { -10.0 + y / plot_height * 130.0 } else { -10.0 }
}

/// Adds `delta` to the band's trim, clamped to ±20 dB.
pub fn nudge_trim(p: &mut HearingProfile, ear: Ear, band: usize, delta: f64) {
    let mut t = p.trim(ear).to_vec();
    let Some(v) = t.get_mut(band) else { return };
    *v = (*v + delta).clamp(-20.0, 20.0);
    p.set_trim(ear, t);
}

/// Sets the band's trim back to 0 dB.
pub fn reset_trim(p: &mut HearingProfile, ear: Ear, band: usize) {
    let mut t = p.trim(ear).to_vec();
    let Some(v) = t.get_mut(band) else { return };
    *v = 0.0;
    p.set_trim(ear, t);
}
