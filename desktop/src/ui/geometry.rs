//! Pure plot geometry for the audiogram and the gain chart (Swift `PlotGeometry` and
//! `GainGeometry`). No egui types, so the maths is testable on its own.

use std::ops::RangeInclusive;

use audiocurve_core::BAND_COUNT;
use audiocurve_core::profile::{Audiogram, Ear};

use crate::model::editor::TrimSelection;

const LEFT_INSET: f64 = 40.0;
const RIGHT_INSET: f64 = 20.0;
const TOP_INSET: f64 = 16.0;
const BOTTOM_INSET: f64 = 28.0;

/// How close (in points) a click must be to a marker or a frequency column to hit it.
pub const HIT_RADIUS: f64 = 12.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub min_x: f64,
    pub min_y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    pub fn max_x(&self) -> f64 {
        self.min_x + self.width
    }

    pub fn max_y(&self) -> f64 {
        self.min_y + self.height
    }

    pub fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.min_x && x <= self.max_x() && y >= self.min_y && y <= self.max_y()
    }

    /// Grows (positive) or shrinks (negative) every side by `d`.
    pub fn inflate(&self, d: f64) -> Rect {
        Rect {
            min_x: self.min_x - d,
            min_y: self.min_y - d,
            width: self.width + 2.0 * d,
            height: self.height + 2.0 * d,
        }
    }
}

fn plot_rect(width: f64, height: f64) -> Rect {
    Rect {
        min_x: LEFT_INSET,
        min_y: TOP_INSET,
        width: (width - LEFT_INSET - RIGHT_INSET).max(1.0),
        height: (height - TOP_INSET - BOTTOM_INSET).max(1.0),
    }
}

/// The audiogram plot: 125…8000 Hz (log) across, −10…120 dB HL (inverted) down.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlotGeometry {
    pub width: f64,
    pub height: f64,
    pub plot: Rect,
}

impl PlotGeometry {
    pub const OCTAVES: f64 = 6.0;

    pub fn new(width: f64, height: f64) -> PlotGeometry {
        PlotGeometry {
            width,
            height,
            plot: plot_rect(width, height),
        }
    }

    pub fn x(&self, f: f64) -> f64 {
        self.plot.min_x + (f / 125.0).log2() / Self::OCTAVES * self.plot.width
    }

    pub fn y(&self, v: f64) -> f64 {
        self.plot.min_y + (v + 10.0) / 130.0 * self.plot.height
    }

    /// The standard frequency whose column is nearest to the point, if the point is within
    /// [`HIT_RADIUS`] of the plot and of that column.
    pub fn hit(&self, x: f64, y: f64) -> Option<f64> {
        if !self.plot.inflate(HIT_RADIUS).contains(x, y) {
            return None;
        }
        Audiogram::STANDARD_FREQUENCIES
            .iter()
            .map(|&f| (f, (self.x(f) - x).abs()))
            .filter(|&(_, d)| d <= HIT_RADIUS)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(f, _)| f)
    }
}

/// The gain chart: one slot per band, a left and a right bar in each, over a dB range.
#[derive(Clone, Debug, PartialEq)]
pub struct GainGeometry {
    pub width: f64,
    pub height: f64,
    pub plot: Rect,
    pub range: RangeInclusive<f64>,
}

impl GainGeometry {
    pub fn new(width: f64, height: f64, range: RangeInclusive<f64>) -> GainGeometry {
        GainGeometry {
            width,
            height,
            plot: plot_rect(width, height),
            range,
        }
    }

    pub fn slot_width(&self) -> f64 {
        self.plot.width / BAND_COUNT as f64
    }

    pub fn bar_width(&self) -> f64 {
        (self.slot_width() * 0.36).min(18.0)
    }

    pub fn x(&self, band: usize) -> f64 {
        self.plot.min_x + self.slot_width() * (band as f64 + 0.5)
    }

    pub fn y(&self, v: f64) -> f64 {
        let span = self.range.end() - self.range.start();
        let span = if span > 0.0 { span } else { 1.0 };
        self.plot.max_y() - (v - self.range.start()) / span * self.plot.height
    }

    /// The bar column (ear and band) under a point inside the plot, with a little slack
    /// above and below.
    pub fn hit(&self, x: f64, y: f64) -> Option<TrimSelection> {
        let p = &self.plot;
        if x < p.min_x || x > p.max_x() || y < p.min_y - 12.0 || y > p.max_y() + 12.0 {
            return None;
        }
        let band = (((x - p.min_x) / self.slot_width()) as usize).min(BAND_COUNT - 1);
        Some(TrimSelection {
            ear: if x < self.x(band) {
                Ear::Left
            } else {
                Ear::Right
            },
            band,
        })
    }
}
