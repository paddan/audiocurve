#![allow(dead_code)]
//! Offline sound-quality measurement of the kernel: distortion, intermodulation, gain
//! modulation, safety-stage activity, inter-sample overs and spectral tilt, on a mild and a
//! steep example audiogram. Shared by `tests/measurements.rs` and `examples/measure.rs`.

#[path = "../mod.rs"]
pub mod support;
pub mod metrics;
pub mod prescription;
pub mod report;
pub mod signals;

use audiocurve_dsp::Params;
use std::collections::BTreeMap;
use std::path::Path;

/// Fictional example audiogram: (frequency Hz, hearing level dB HL) per ear.
pub struct Audiogram {
    pub left: &'static [(f64, f64)],
    pub right: &'static [(f64, f64)],
}

const MILD_PTS: &[(f64, f64)] =
    &[(125., 10.), (250., 10.), (500., 15.), (1000., 20.), (2000., 30.), (4000., 40.), (8000., 45.)];
pub const MILD: Audiogram = Audiogram { left: MILD_PTS, right: MILD_PTS };

pub const STEEP: Audiogram = Audiogram {
    left: &[
        (125., 20.), (250., 20.), (500., 30.), (1000., 45.), (2000., 60.),
        (3000., 70.), (4000., 80.), (6000., 90.), (8000., 90.),
    ],
    // Left minus 20 dB, floored at 0.
    right: &[
        (125., 0.), (250., 0.), (500., 10.), (1000., 25.), (2000., 40.),
        (3000., 50.), (4000., 60.), (6000., 70.), (8000., 70.),
    ],
};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Speech,
    Music,
    Auto,
}

impl Mode {
    pub fn name(self) -> &'static str {
        match self {
            Mode::Speech => "speech",
            Mode::Music => "music",
            Mode::Auto => "auto",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ear {
    Left,
    Right,
}

/// Kernel parameters for an audiogram and mode: MPO -3 dBFS, unity gains, limiter on.
pub fn params(a: &Audiogram, mode: Mode) -> Params {
    let mut p = Params::unity();
    p.left = prescription::fittings(a, Ear::Left, mode);
    p.right = prescription::fittings(a, Ear::Right, mode);
    p.mpo_dbfs = -3.0;
    p.output_gain_db = 0.0;
    p.input_gain_db = 0.0;
    p.limiter_enabled = 1.0;
    p.active = 1.0;
    let (attack, release, auto) = match mode {
        Mode::Speech => (5.0, 50.0, 0.0),
        Mode::Music => (10.0, 200.0, 0.0),
        Mode::Auto => (10.0, 200.0, 1.0),
    };
    p.attack_ms = attack;
    p.release_ms = release;
    p.auto_mode = auto;
    p
}

/// Runs every metric for both audiograms. Keys are `"{mild|steep}/{speech|music|auto}/{metric}"`.
/// With `write_wav`, input/output WAV pairs are written to that directory.
pub fn run_all(write_wav: Option<&Path>) -> BTreeMap<String, f64> {
    let mut out = BTreeMap::new();
    std::thread::scope(|s| {
        let mut jobs = Vec::new();
        for (aname, a) in [("mild", &MILD), ("steep", &STEEP)] {
            for mode in [Mode::Speech, Mode::Music, Mode::Auto] {
                jobs.push((aname, mode, s.spawn(move || metrics::measure(aname, a, mode, write_wav))));
            }
        }
        for (aname, mode, job) in jobs {
            for (metric, v) in job.join().expect("measurement thread panicked") {
                out.insert(format!("{aname}/{}/{metric}", mode.name()), v);
            }
        }
    });
    out
}
