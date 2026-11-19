//! Clinical-style ascending/descending threshold search, 5 dB HL steps (Swift `HearingTest`).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::TestCalibration;
use crate::profile::{Audiogram, AudiogramPoint, Ear};

/// One tone (or silent catch trial) to play to the listener.
#[derive(Clone, Debug, PartialEq)]
pub struct Presentation {
    pub ear: Ear,
    pub frequency_hz: f64,
    pub level_hl: f64,
    /// None = catch trial: stay silent.
    pub level_dbfs: Option<f64>,
    /// Silence before the tone, in seconds.
    pub gap_before_s: f64,
    /// 1-based point number (1..=TOTAL_POINTS), for progress.
    pub index: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum TestFlag {
    RetestMismatch { ear: Ear, db: f64 },
    CatchTrialPresses(u32),
    NoResponseAtMax { ear: Ear, frequency_hz: f64 },
    NotConverged { ear: Ear, frequency_hz: f64 },
    Asymmetry,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct TestPoint {
    pub frequency_hz: f64,
    /// Raw presented level of the threshold.
    pub dbfs: f64,
    /// Threshold in dB HL, rounded.
    pub hl: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct HearingTestResult {
    /// Sorted by frequency, one point per standard frequency.
    pub left: Vec<TestPoint>,
    pub right: Vec<TestPoint>,
    pub flags: Vec<TestFlag>,
}

impl HearingTestResult {
    pub fn audiogram(&self, ear: Ear) -> Audiogram {
        let points = match ear {
            Ear::Left => &self.left,
            Ear::Right => &self.right,
        };
        Audiogram {
            points: points.iter().map(|p| AudiogramPoint { frequency_hz: p.frequency_hz, threshold_db_hl: p.hl }).collect(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum HearingTestStep {
    Present(Presentation),
    Finished(HearingTestResult),
}

/// Staircase state for one (ear, frequency) point.
#[derive(Clone, Debug)]
struct Staircase {
    max_hl: f64,
    level: f64,
    ascending: bool,
    heard_before: bool,
    presentations: u32,
    not_heard_at_max: u32,
    heard_at_floor: u32,
    /// Ascending hits per level.
    ascending_heard_counts: Vec<(f64, u32)>,
    lowest_heard: Option<f64>,
    lowest_ascending_heard: Option<f64>,
}

impl Staircase {
    fn new(calibration: &TestCalibration, frequency_hz: f64) -> Self {
        let max_hl = calibration.max_hl(frequency_hz);
        Self {
            max_hl,
            level: max_hl.min(40.0),
            ascending: false,
            heard_before: false,
            presentations: 0,
            not_heard_at_max: 0,
            heard_at_floor: 0,
            ascending_heard_counts: Vec::new(),
            lowest_heard: None,
            lowest_ascending_heard: None,
        }
    }

    /// Adds an ascending hit at `level` and returns the new count.
    fn count_ascending_hit(&mut self, level: f64) -> u32 {
        if let Some(entry) = self.ascending_heard_counts.iter_mut().find(|e| e.0 == level) {
            entry.1 += 1;
            entry.1
        } else {
            self.ascending_heard_counts.push((level, 1));
            1
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Outcome {
    level_hl: f64,
    no_response_at_max: bool,
    not_converged: bool,
}

impl Outcome {
    fn at(level_hl: f64) -> Self {
        Self { level_hl, no_response_at_max: false, not_converged: false }
    }
}

fn min_opt(a: Option<f64>, b: f64) -> f64 {
    a.map_or(b, |a| a.min(b))
}

pub struct HearingTest {
    calibration: TestCalibration,
    random: Box<dyn FnMut() -> f64>,
    position: usize,
    staircase: Staircase,
    pending: Option<Presentation>,
    pending_is_catch: bool,
    last_was_catch: bool,
    catch_presses: u32,
    thresholds: BTreeMap<Ear, BTreeMap<usize, Outcome>>,
    flags: Vec<TestFlag>,
    result: Option<HearingTestResult>,
}

impl HearingTest {
    pub const ORDER: [f64; 12] =
        [1000.0, 1500.0, 2000.0, 3000.0, 4000.0, 6000.0, 8000.0, 1000.0, 750.0, 500.0, 250.0, 125.0];
    pub const TOTAL_POINTS: usize = 24;

    const EARS: [Ear; 2] = [Ear::Right, Ear::Left];
    const FLOOR_HL: f64 = -10.0;
    const MAX_PRESENTATIONS_PER_POINT: u32 = 12;
    const CATCH_PROBABILITY: f64 = 1.0 / 8.0;
    /// Order index of the second 1 kHz run, retested against index 0.
    const RETEST_INDEX: usize = 7;

    pub fn new(calibration: TestCalibration, random: Box<dyn FnMut() -> f64>) -> Self {
        let staircase = Staircase::new(&calibration, Self::ORDER[0]);
        Self {
            calibration,
            random,
            position: 0,
            staircase,
            pending: None,
            pending_is_catch: false,
            last_was_catch: false,
            catch_presses: 0,
            thresholds: BTreeMap::new(),
            flags: Vec::new(),
            result: None,
        }
    }

    fn ear(&self) -> Ear {
        Self::EARS[self.position / Self::ORDER.len()]
    }

    fn frequency_hz(&self) -> f64 {
        Self::ORDER[self.position % Self::ORDER.len()]
    }

    /// The current presentation (the same one until it is answered), or the result.
    #[allow(clippy::should_implement_trait)] // named after the Swift API; not an iterator
    pub fn next(&mut self) -> HearingTestStep {
        if let Some(result) = &self.result {
            return HearingTestStep::Finished(result.clone());
        }
        if let Some(p) = &self.pending {
            return HearingTestStep::Present(p.clone());
        }
        let is_catch = !self.last_was_catch && (self.random)() < Self::CATCH_PROBABILITY;
        let gap = 1.5 + 2.0 * (self.random)();
        self.pending_is_catch = is_catch;
        let f = self.frequency_hz();
        let p = Presentation {
            ear: self.ear(),
            frequency_hz: f,
            level_hl: self.staircase.level,
            level_dbfs: (!is_catch).then(|| {
                TestCalibration::cap_dbfs(f).min(self.calibration.dbfs_from_hl(self.staircase.level, f))
            }),
            gap_before_s: gap,
            index: self.position + 1,
        };
        self.pending = Some(p.clone());
        HearingTestStep::Present(p)
    }

    /// Answers the current presentation. Ignored when there is none outstanding.
    pub fn respond(&mut self, heard: bool) {
        if self.pending.take().is_none() {
            return;
        }
        self.last_was_catch = self.pending_is_catch;
        if self.pending_is_catch {
            if heard {
                self.catch_presses += 1;
            }
            return;
        }
        if let Some(outcome) = self.advance(heard) {
            self.finish_point(outcome);
        }
    }

    /// Moves the staircase; returns the outcome once the point is resolved.
    fn advance(&mut self, heard: bool) -> Option<Outcome> {
        let s = &mut self.staircase;
        let level = s.level;
        s.presentations += 1;
        let mut outcome = None;

        if heard {
            s.heard_before = true;
            s.lowest_heard = Some(min_opt(s.lowest_heard, level));
            if s.ascending {
                let count = s.count_ascending_hit(level);
                s.lowest_ascending_heard = Some(min_opt(s.lowest_ascending_heard, level));
                if count >= 2 {
                    outcome = Some(Outcome::at(level));
                }
            }
            if level <= Self::FLOOR_HL {
                s.heard_at_floor += 1;
                if outcome.is_none() && s.heard_at_floor >= 2 {
                    outcome = Some(Outcome::at(level));
                }
            }
            s.level = (level - 10.0).max(Self::FLOOR_HL).min(s.max_hl);
            s.ascending = false;
        } else if level >= s.max_hl {
            s.not_heard_at_max += 1;
            if s.not_heard_at_max >= 2 {
                outcome = Some(Outcome { no_response_at_max: true, ..Outcome::at(s.max_hl) });
            }
            s.level = s.max_hl;
            s.ascending = true;
        } else if s.heard_before {
            s.level = s.max_hl.min(level + 5.0);
            s.ascending = true;
        } else {
            let step = if level >= 80.0 { 5.0 } else { 20.0 }; // steps above 80 are +5
            s.level = s.max_hl.min(level + step);
            s.ascending = step == 5.0;
        }

        if outcome.is_none() && s.presentations >= Self::MAX_PRESENTATIONS_PER_POINT {
            let level = s.lowest_ascending_heard.or(s.lowest_heard).unwrap_or(s.max_hl);
            outcome = Some(Outcome { not_converged: true, ..Outcome::at(level) });
        }
        outcome
    }

    fn finish_point(&mut self, outcome: Outcome) {
        let ear = self.ear();
        let f = self.frequency_hz();
        let index = self.position % Self::ORDER.len();
        let ear_thresholds = self.thresholds.entry(ear).or_default();
        ear_thresholds.insert(index, outcome);
        if outcome.no_response_at_max {
            self.flags.push(TestFlag::NoResponseAtMax { ear, frequency_hz: f });
        }
        if outcome.not_converged {
            self.flags.push(TestFlag::NotConverged { ear, frequency_hz: f });
        }
        if index == Self::RETEST_INDEX
            && let Some(first) = ear_thresholds.get(&0)
        {
            let diff = (first.level_hl - outcome.level_hl).abs();
            if diff > 10.0 {
                self.flags.push(TestFlag::RetestMismatch { ear, db: diff });
            }
        }

        self.position += 1;
        if self.position == Self::TOTAL_POINTS {
            self.result = Some(self.make_result());
        } else {
            self.staircase = Staircase::new(&self.calibration, self.frequency_hz());
        }
    }

    fn points(&self, ear: Ear) -> Vec<TestPoint> {
        // (frequency, level); the 1 kHz value is the lower of the two runs.
        let mut by_frequency: Vec<(f64, f64)> = Vec::new();
        for (&i, outcome) in self.thresholds.get(&ear).into_iter().flatten() {
            let f = Self::ORDER[i];
            match by_frequency.iter_mut().find(|e| e.0 == f) {
                Some(e) => e.1 = e.1.min(outcome.level_hl),
                None => by_frequency.push((f, outcome.level_hl)),
            }
        }
        by_frequency.sort_by(|a, b| a.0.total_cmp(&b.0));
        by_frequency
            .into_iter()
            .map(|(f, level)| TestPoint {
                frequency_hz: f,
                dbfs: self.calibration.dbfs_from_hl(level, f),
                hl: TestCalibration::rounded_hl(level),
            })
            .collect()
    }

    fn make_result(&self) -> HearingTestResult {
        let left = self.points(Ear::Left);
        let right = self.points(Ear::Right);
        let mut flags = self.flags.clone();
        if self.catch_presses > 2 {
            flags.push(TestFlag::CatchTrialPresses(self.catch_presses));
        }
        if left.iter().zip(&right).any(|(l, r)| (l.hl - r.hl).abs() >= 40.0) {
            flags.push(TestFlag::Asymmetry);
        }
        HearingTestResult { left, right, flags }
    }
}
