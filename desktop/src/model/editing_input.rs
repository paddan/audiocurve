//! Keyboard and pointer editing of the audiogram and the band trims (Swift
//! `PluginViewModel.handleKey`/`beginDrag`/`drag`/`endDrag`/`resetTrim` and `Fmt`).
//! Every change to the profile goes through `Editor::set_profile`, so autosave keeps working.

use std::ops::RangeInclusive;
use std::time::Instant;

use audiocurve_core::editing::{self, MIN_POINTS, PointSelection};
use audiocurve_core::profile::{Ear, HearingProfile};
use audiocurve_core::{BAND_COUNT, CENTERS};

use crate::i18n::{self, Lang, tr, trf};
use crate::model::editor::{Editor, Pane, TrimSelection};
use crate::ui::geometry::{GainGeometry, HIT_RADIUS, PlotGeometry};

/// The keys the editor reacts to. Modifier handling is the caller's: it drops key presses with
/// Cmd or Ctrl held and passes Option as `fine`. The keypad Enter is `Return`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Up,
    Down,
    Left,
    Right,
    Tab,
    Delete,
    Return,
    Escape,
    Char(char),
}

/// What a running drag moves.
#[derive(Clone, Debug)]
pub(crate) enum DragTarget {
    /// An audiogram point; `offset_y` keeps the point under the pointer where it was grabbed.
    Point { sel: PointSelection, offset_y: f64 },
    /// A trim bar, moved relative to where the drag started, on the gain range frozen then.
    Trim {
        t: TrimSelection,
        start_y: f64,
        start_value: f64,
        range: RangeInclusive<f64>,
    },
}

#[derive(Clone, Debug)]
pub(crate) struct DragState {
    pub target: DragTarget,
    pub start: (f64, f64),
    pub moved: bool,
}

/// Number formatting for labels (Swift `Fmt`).
pub mod fmt {
    use super::*;

    /// Comma in every supported language except English.
    fn decimal() -> &'static str {
        if i18n::lang() == Lang::En { "." } else { "," }
    }

    pub fn freq(hz: f64) -> String {
        if hz < 1000.0 {
            return format!("{} Hz", hz as i64);
        }
        let k = hz / 1000.0;
        if k == k.round() {
            format!("{k:.0} kHz")
        } else {
            format!("{k:.1} kHz").replace('.', decimal())
        }
    }

    /// Axis tick: "500", "1k", "1.5k".
    pub fn axis_freq(hz: f64) -> String {
        if hz < 1000.0 {
            return format!("{}", hz as i64);
        }
        let k = hz / 1000.0;
        if k == k.round() {
            format!("{k:.0}k")
        } else {
            format!("{k:.1}k").replace('.', decimal())
        }
    }

    /// Whole numbers without decimals, otherwise one decimal; regional separator and true minus.
    pub fn db(v: f64) -> String {
        let r = (v * 10.0).round() / 10.0;
        let s = if r == r.round() {
            format!("{:.0}", r.abs())
        } else {
            format!("{:.1}", r.abs()).replace('.', decimal())
        };
        if r < 0.0 { format!("\u{2212}{s}") } else { s }
    }

    pub fn signed(v: f64) -> String {
        let s = db(v);
        if v > 0.0 { format!("+{s}") } else { s }
    }

    pub fn ear_initial(e: Ear) -> String {
        tr(if e == Ear::Left { "L" } else { "R" })
    }

    pub fn ear_name(e: Ear) -> String {
        tr(if e == Ear::Left { "Left" } else { "Right" })
    }
}

impl Editor {
    /// Handles a key press. Returns true when it was used. Digits and the minus sign go to the
    /// typed buffer of the selected audiogram point; Return commits it.
    pub fn handle_key(&mut self, key: Key, fine: bool, now: Instant) -> bool {
        if self.profile.is_none() {
            return false;
        }
        match self.pane {
            Pane::Spectrum => false,
            Pane::Audiogram => self.audiogram_key(key, fine, now),
            Pane::Gain => self.gain_key(key, now),
        }
    }

    fn first_point(&self, ear: Ear) -> Option<PointSelection> {
        let p = self.profile.as_ref()?;
        p.audiogram(ear).points.first().map(|q| PointSelection {
            ear,
            frequency_hz: q.frequency_hz,
        })
    }

    fn audiogram_key(&mut self, key: Key, fine: bool, now: Instant) -> bool {
        let Some(mut p) = self.profile.clone() else {
            return false;
        };
        match key {
            Key::Escape => {
                self.selection = None;
                self.typed_buffer.clear();
            }
            Key::Up | Key::Down => {
                let Some(s) = self.selection else {
                    self.selection = self.first_point(self.active_ear);
                    return true;
                };
                self.typed_buffer.clear();
                editing::nudge(&mut p, s, key == Key::Up, fine);
                self.set_profile(p, now);
            }
            Key::Left | Key::Right => {
                let Some(s) = self.selection else {
                    let pts = &p.audiogram(self.active_ear).points;
                    let pick = if key == Key::Left {
                        pts.last()
                    } else {
                        pts.first()
                    };
                    self.selection = pick.map(|q| PointSelection {
                        ear: self.active_ear,
                        frequency_hz: q.frequency_hz,
                    });
                    return true;
                };
                self.typed_buffer.clear();
                if let Some(n) = editing::neighbor(&p, s, key == Key::Right) {
                    self.selection = Some(n);
                }
            }
            Key::Tab => {
                self.typed_buffer.clear();
                if let Some(s) = self.selection {
                    let n = editing::switch_ear(&p, s);
                    self.selection = Some(n);
                    self.active_ear = n.ear;
                } else {
                    self.active_ear = self.active_ear.other();
                    self.selection = self.first_point(self.active_ear);
                }
            }
            Key::Delete => {
                if self.typed_buffer.pop().is_some() {
                    return true;
                }
                let Some(s) = self.selection else { return true };
                let next =
                    editing::neighbor(&p, s, false).or_else(|| editing::neighbor(&p, s, true));
                if editing::delete(&mut p, s) {
                    self.selection = next;
                    self.set_profile(p, now);
                } else {
                    self.show_error(trf(
                        "At least %lld points per ear",
                        &[&MIN_POINTS.to_string()],
                    ));
                }
            }
            Key::Return => self.commit_typed(p, now),
            Key::Char(c) => {
                if self.selection.is_none() {
                    return false;
                }
                if c.is_ascii_digit() {
                    if self.typed_buffer.chars().count() < 4 {
                        self.typed_buffer.push(c);
                    }
                } else if c == '-' || c == '\u{2212}' {
                    if self.typed_buffer.is_empty() {
                        self.typed_buffer.push('-');
                    }
                } else {
                    return false;
                }
            }
        }
        true
    }

    fn commit_typed(&mut self, mut p: HearingProfile, now: Instant) {
        let Some(s) = self.selection.filter(|_| !self.typed_buffer.is_empty()) else {
            return;
        };
        match editing::parse_typed(&self.typed_buffer) {
            Some(v) => {
                editing::set_value(&mut p, s, v, true);
                self.set_profile(p, now);
            }
            None => self.show_error(tr("Enter a value between −10 and 120 dB HL")),
        }
        self.typed_buffer.clear();
    }

    fn gain_key(&mut self, key: Key, now: Instant) -> bool {
        let Some(mut p) = self.profile.clone() else {
            return false;
        };
        match key {
            Key::Escape => self.trim_selection = None,
            Key::Up | Key::Down => {
                let Some(t) = self.trim_selection else {
                    self.trim_selection = Some(TrimSelection {
                        ear: self.active_ear,
                        band: 0,
                    });
                    return true;
                };
                editing::nudge_trim(
                    &mut p,
                    t.ear,
                    t.band,
                    if key == Key::Up { 1.0 } else { -1.0 },
                );
                self.set_profile(p, now);
            }
            Key::Left | Key::Right => {
                let Some(t) = self.trim_selection else {
                    self.trim_selection = Some(TrimSelection {
                        ear: self.active_ear,
                        band: 0,
                    });
                    return true;
                };
                let band = if key == Key::Right {
                    (t.band + 1).min(BAND_COUNT - 1)
                } else {
                    t.band.saturating_sub(1)
                };
                self.trim_selection = Some(TrimSelection { ear: t.ear, band });
            }
            Key::Tab => {
                let ear = self
                    .trim_selection
                    .map_or(self.active_ear, |t| t.ear)
                    .other();
                self.trim_selection = Some(TrimSelection {
                    ear,
                    band: self.trim_selection.map_or(0, |t| t.band),
                });
                self.active_ear = ear;
            }
            Key::Delete => {
                let Some(t) = self.trim_selection else {
                    return true;
                };
                editing::reset_trim(&mut p, t.ear, t.band);
                self.set_profile(p, now);
            }
            Key::Return | Key::Char(_) => return false,
        }
        true
    }

    /// Mouse down at (x, y) in a plot of size w × h. In the audiogram it grabs the nearest point
    /// (active ear first) or adds one at a standard frequency; in the gain chart it grabs a bar.
    /// `gain_range` is the axis range to freeze for the whole drag.
    pub fn begin_drag(
        &mut self,
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        gain_range: RangeInclusive<f64>,
        now: Instant,
    ) {
        self.drag = None;
        let Some(p) = self.profile.clone() else {
            return;
        };
        self.typed_buffer.clear();
        let start = (x, y);
        match self.pane {
            Pane::Audiogram => self.begin_point_drag(p, start, w, h, now),
            Pane::Gain => {
                let g = GainGeometry::new(w, h, gain_range.clone());
                let Some(t) = g.hit(x, y) else {
                    self.trim_selection = None;
                    return;
                };
                self.trim_selection = Some(t);
                self.active_ear = t.ear;
                let target = DragTarget::Trim {
                    t,
                    start_y: y,
                    start_value: p.trim(t.ear)[t.band],
                    range: gain_range,
                };
                self.drag = Some(DragState {
                    target,
                    start,
                    moved: false,
                });
            }
            Pane::Spectrum => {}
        }
    }

    fn begin_point_drag(
        &mut self,
        p: HearingProfile,
        start: (f64, f64),
        w: f64,
        h: f64,
        now: Instant,
    ) {
        let (x, y) = start;
        let g = PlotGeometry::new(w, h);
        for ear in [self.active_ear, self.active_ear.other()] {
            let nearest = p
                .audiogram(ear)
                .points
                .iter()
                .map(|q| {
                    (
                        q,
                        (g.x(q.frequency_hz) - x).hypot(g.y(q.threshold_db_hl) - y),
                    )
                })
                .filter(|&(_, d)| d <= HIT_RADIUS)
                .min_by(|a, b| a.1.total_cmp(&b.1));
            if let Some((hit, _)) = nearest {
                let sel = PointSelection {
                    ear,
                    frequency_hz: hit.frequency_hz,
                };
                self.selection = Some(sel);
                self.active_ear = ear;
                let target = DragTarget::Point {
                    sel,
                    offset_y: g.y(hit.threshold_db_hl) - y,
                };
                self.drag = Some(DragState {
                    target,
                    start,
                    moved: false,
                });
                return;
            }
        }
        let Some(f) = g.hit(x, y) else {
            self.selection = None;
            return;
        };
        let sel = PointSelection {
            ear: self.active_ear,
            frequency_hz: f,
        };
        self.selection = Some(sel);
        if let Some(existing) = p
            .audiogram(self.active_ear)
            .points
            .iter()
            .find(|q| q.frequency_hz == f)
        {
            let target = DragTarget::Point {
                sel,
                offset_y: g.y(existing.threshold_db_hl) - y,
            };
            self.drag = Some(DragState {
                target,
                start,
                moved: false,
            });
        } else {
            let mut q = p;
            let v = editing::db_hl_at_y(y - g.plot.min_y, g.plot.height);
            editing::set_value(&mut q, sel, v, false);
            self.set_profile(q, now);
            self.drag = Some(DragState {
                target: DragTarget::Point { sel, offset_y: 0.0 },
                start,
                moved: true,
            });
        }
    }

    /// Pointer moved with the button down. Ignored until it has moved 2 points from the start.
    pub fn drag(&mut self, x: f64, y: f64, w: f64, h: f64, fine: bool, now: Instant) {
        let Some(state) = self.drag.as_mut() else {
            return;
        };
        let Some(mut p) = self.profile.clone() else {
            return;
        };
        if !state.moved {
            if (x - state.start.0).hypot(y - state.start.1) < 2.0 {
                return;
            }
            state.moved = true;
        }
        match state.target.clone() {
            DragTarget::Point { sel, offset_y } => {
                let g = PlotGeometry::new(w, h);
                let v = editing::db_hl_at_y(y + offset_y - g.plot.min_y, g.plot.height);
                editing::set_value(&mut p, sel, v, fine);
            }
            DragTarget::Trim {
                t,
                start_y,
                start_value,
                range,
            } => {
                let g = GainGeometry::new(w, h, range.clone());
                let per_point = (range.end() - range.start()) / g.plot.height;
                let target = (start_value + (start_y - y) * per_point)
                    .round()
                    .clamp(-20.0, 20.0);
                let Some(&current) = p.trim(t.ear).get(t.band) else {
                    return;
                };
                editing::nudge_trim(&mut p, t.ear, t.band, target - current);
            }
        }
        self.set_profile(p, now);
    }

    pub fn end_drag(&mut self) {
        self.drag = None;
    }

    /// Double click in the gain chart: sets that bar's trim back to 0.
    pub fn reset_trim_at(
        &mut self,
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        gain_range: RangeInclusive<f64>,
        now: Instant,
    ) {
        if self.pane != Pane::Gain {
            return;
        }
        let Some(mut p) = self.profile.clone() else {
            return;
        };
        let Some(t) = GainGeometry::new(w, h, gain_range).hit(x, y) else {
            return;
        };
        self.trim_selection = Some(t);
        self.active_ear = t.ear;
        editing::reset_trim(&mut p, t.ear, t.band);
        self.set_profile(p, now);
    }

    /// "L 1 kHz · 45 dB HL"; the typed digits and a cursor replace the value while typing.
    /// Empty if the point does not exist.
    pub fn point_label(&self, s: PointSelection) -> String {
        let Some(p) = &self.profile else {
            return String::new();
        };
        let Some(pt) = p
            .audiogram(s.ear)
            .points
            .iter()
            .find(|q| q.frequency_hz == s.frequency_hz)
        else {
            return String::new();
        };
        let value = if self.typed_buffer.is_empty() {
            fmt::db(pt.threshold_db_hl)
        } else {
            format!("{}_", self.typed_buffer)
        };
        format!(
            "{} {} · {} dB HL",
            fmt::ear_initial(s.ear),
            fmt::freq(s.frequency_hz),
            value
        )
    }

    /// "L 1 kHz · trim +4 dB · gain 12.5 dB"; `gain_soft_db` is the band's gain in the fitting
    /// in effect, which the caller computes.
    pub fn trim_label(&self, t: TrimSelection, gain_soft_db: f64) -> String {
        let Some(trim) = self
            .profile
            .as_ref()
            .and_then(|p| p.trim(t.ear).get(t.band).copied())
        else {
            return String::new();
        };
        trf(
            "%@ %@ · trim %@ dB · gain %@ dB",
            &[
                &fmt::ear_initial(t.ear),
                &fmt::freq(CENTERS[t.band]),
                &fmt::signed(trim),
                &fmt::db(gain_soft_db),
            ],
        )
    }
}
