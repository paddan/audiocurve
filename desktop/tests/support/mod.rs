#![allow(dead_code)]
//! Shared test data, copied from core's test support.

use audiocurve_core::profile::{Audiogram, AudiogramPoint, HearingProfile};

/// Fictional, steeply sloping example audiogram (not a real person's).
pub const EXAMPLE_LEFT: [(f64, f64); 11] = [
    (125.0, 20.0),
    (250.0, 25.0),
    (500.0, 35.0),
    (750.0, 40.0),
    (1000.0, 55.0),
    (1500.0, 65.0),
    (2000.0, 70.0),
    (3000.0, 80.0),
    (4000.0, 85.0),
    (6000.0, 95.0),
    (8000.0, 105.0),
];
pub const EXAMPLE_RIGHT: [(f64, f64); 11] = [
    (125.0, 15.0),
    (250.0, 15.0),
    (500.0, 20.0),
    (750.0, 25.0),
    (1000.0, 35.0),
    (1500.0, 45.0),
    (2000.0, 50.0),
    (3000.0, 60.0),
    (4000.0, 65.0),
    (6000.0, 75.0),
    (8000.0, 85.0),
];

pub fn audiogram_from(points: &[(f64, f64)]) -> Audiogram {
    let mut pts = points.to_vec();
    pts.sort_by(|a, b| a.0.total_cmp(&b.0));
    Audiogram {
        points: pts
            .into_iter()
            .map(|(f, t)| AudiogramPoint {
                frequency_hz: f,
                threshold_db_hl: t,
            })
            .collect(),
    }
}

pub fn example_profile() -> HearingProfile {
    HearingProfile {
        left: audiogram_from(&EXAMPLE_LEFT),
        right: audiogram_from(&EXAMPLE_RIGHT),
        left_trim_db: vec![0.0; 12],
        right_trim_db: vec![0.0; 12],
    }
}
