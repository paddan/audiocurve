//! Auto level (Swift `PluginViewModel.startAutoLevel`): reads the meters every 50 ms for 60
//! blocks (3 s) and derives the output gain from them. Time is passed in by the caller.

use std::time::{Duration, Instant};

use crate::model::levels::Levels;
use audiocurve_core::auto_level::{AutoLevelMeasurement, AutoLevelResult};
use audiocurve_dsp::Meters;

pub const BLOCKS: u32 = 60;
pub const BLOCK_INTERVAL: Duration = Duration::from_millis(50);

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AutoLevelUi {
    Idle,
    /// Blocks still to read.
    Measuring {
        remaining: u32,
    },
    /// The output gain that was set, in dB.
    Done(f64),
    TooQuiet,
}

pub(crate) struct AutoLevel {
    pub ui: AutoLevelUi,
    measurement: AutoLevelMeasurement,
    next_at: Instant,
}

impl AutoLevel {
    pub fn new(now: Instant) -> AutoLevel {
        AutoLevel {
            ui: AutoLevelUi::Idle,
            measurement: AutoLevelMeasurement::default(),
            next_at: now,
        }
    }

    pub fn is_measuring(&self) -> bool {
        matches!(self.ui, AutoLevelUi::Measuring { .. })
    }

    pub fn start(&mut self, now: Instant) {
        if self.is_measuring() {
            return;
        }
        self.measurement = AutoLevelMeasurement::default();
        self.ui = AutoLevelUi::Measuring { remaining: BLOCKS };
        self.next_at = now;
    }

    pub fn next_deadline(&self) -> Option<Instant> {
        self.is_measuring().then_some(self.next_at)
    }

    /// Reads one block when it is due. After the last block, returns the gain to set (rounded
    /// to 0.1 dB), or None when the input was too quiet.
    pub fn sample(
        &mut self,
        now: Instant,
        meters: impl FnOnce() -> Meters,
        levels: &Levels,
    ) -> Option<f64> {
        let AutoLevelUi::Measuring { remaining } = self.ui else {
            return None;
        };
        if now < self.next_at {
            return None;
        }
        self.measurement.add(&meters());
        self.next_at = now + BLOCK_INTERVAL;
        let remaining = remaining - 1;
        if remaining > 0 {
            self.ui = AutoLevelUi::Measuring { remaining };
            return None;
        }
        match self
            .measurement
            .result(levels.output_gain_db, levels.mpo_dbfs)
        {
            AutoLevelResult::TooQuiet => {
                self.ui = AutoLevelUi::TooQuiet;
                None
            }
            AutoLevelResult::Gain(g) => {
                let rounded = (g * 10.0).round() / 10.0;
                self.ui = AutoLevelUi::Done(rounded);
                Some(rounded)
            }
        }
    }
}
