//! Audiograms and hearing profiles (Swift `Audiogram`, `Bands`, `HearingProfile`).

use serde::{Deserialize, Serialize};

use crate::BAND_COUNT;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Ear {
    Left,
    Right,
}

impl Ear {
    pub const ALL: [Ear; 2] = [Ear::Left, Ear::Right];

    pub fn other(self) -> Ear {
        match self {
            Ear::Left => Ear::Right,
            Ear::Right => Ear::Left,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct AudiogramPoint {
    pub frequency_hz: f64,
    pub threshold_db_hl: f64,
}

/// Thresholds sorted by ascending frequency.
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct Audiogram {
    pub points: Vec<AudiogramPoint>,
}

impl Audiogram {
    pub const STANDARD_FREQUENCIES: [f64; 11] =
        [125.0, 250.0, 500.0, 750.0, 1000.0, 1500.0, 2000.0, 3000.0, 4000.0, 6000.0, 8000.0];
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ProfileValidationError {
    TooFewPoints(Ear),
    UnsortedOrDuplicateFrequencies(Ear),
    NonStandardFrequency(Ear, f64),
    ThresholdOutOfRange { ear: Ear, frequency_hz: f64 },
    WrongTrimCount(Ear),
    TrimOutOfRange { ear: Ear, band: usize },
}

impl std::fmt::Display for ProfileValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for ProfileValidationError {}

/// Both ears' audiograms and a per-band trim in dB (12 values, ±20).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HearingProfile {
    pub left: Audiogram,
    pub right: Audiogram,
    pub left_trim_db: Vec<f64>,
    pub right_trim_db: Vec<f64>,
}

impl HearingProfile {
    /// Normal hearing: 0 dB HL at all standard frequencies for both ears, no trim.
    pub fn flat() -> Self {
        let a = Audiogram {
            points: Audiogram::STANDARD_FREQUENCIES
                .iter()
                .map(|&f| AudiogramPoint { frequency_hz: f, threshold_db_hl: 0.0 })
                .collect(),
        };
        Self { left: a.clone(), right: a, left_trim_db: vec![0.0; BAND_COUNT], right_trim_db: vec![0.0; BAND_COUNT] }
    }

    pub fn audiogram(&self, ear: Ear) -> &Audiogram {
        match ear {
            Ear::Left => &self.left,
            Ear::Right => &self.right,
        }
    }

    pub fn set_audiogram(&mut self, ear: Ear, a: Audiogram) {
        match ear {
            Ear::Left => self.left = a,
            Ear::Right => self.right = a,
        }
    }

    pub fn trim(&self, ear: Ear) -> &[f64] {
        match ear {
            Ear::Left => &self.left_trim_db,
            Ear::Right => &self.right_trim_db,
        }
    }

    pub fn set_trim(&mut self, ear: Ear, t: Vec<f64>) {
        match ear {
            Ear::Left => self.left_trim_db = t,
            Ear::Right => self.right_trim_db = t,
        }
    }

    pub fn validate(&self) -> Result<(), ProfileValidationError> {
        validate_ear(&self.left, &self.left_trim_db, Ear::Left)?;
        validate_ear(&self.right, &self.right_trim_db, Ear::Right)
    }
}

fn validate_ear(a: &Audiogram, trim: &[f64], ear: Ear) -> Result<(), ProfileValidationError> {
    use ProfileValidationError as E;
    if a.points.len() < 4 {
        return Err(E::TooFewPoints(ear));
    }
    if a.points.windows(2).any(|w| w[1].frequency_hz.partial_cmp(&w[0].frequency_hz) != Some(std::cmp::Ordering::Greater)) {
        return Err(E::UnsortedOrDuplicateFrequencies(ear));
    }
    for p in &a.points {
        if !Audiogram::STANDARD_FREQUENCIES.contains(&p.frequency_hz) {
            return Err(E::NonStandardFrequency(ear, p.frequency_hz));
        }
        if !(-10.0..=120.0).contains(&p.threshold_db_hl) {
            return Err(E::ThresholdOutOfRange { ear, frequency_hz: p.frequency_hz });
        }
    }
    if trim.len() != BAND_COUNT {
        return Err(E::WrongTrimCount(ear));
    }
    if let Some(band) = trim.iter().position(|t| !(-20.0..=20.0).contains(t)) {
        return Err(E::TrimOutOfRange { ear, band });
    }
    Ok(())
}
