//! Maps digital level (dBFS) to hearing level (dB HL) for the hearing test (Swift `TestCalibration`).

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Transducer {
    InEar,
    OverEar,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CalibrationKind {
    Nominal,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct TestCalibration {
    pub transducer: Transducer,
    /// SPL produced by a full-scale (0 dBFS) sine.
    pub full_scale_spl: f64,
    pub kind: CalibrationKind,
}

/// Reference equivalent threshold SPL (dB) at the standard audiogram frequencies.
/// In-ear: ISO 389-2 (insert earphones, occluded-ear simulator, IEC 60318-4).
/// Over-ear: ISO 389-8 (circumaural earphones, e.g. HDA 200, IEC 60318-2 coupler).
const RETSPL: [(f64, f64, f64); 11] = [
    // (Hz, in-ear, over-ear)
    (125.0, 26.0, 30.5),
    (250.0, 14.0, 18.0),
    (500.0, 5.5, 11.0),
    (750.0, 2.0, 6.0),
    (1000.0, 0.0, 5.5),
    (1500.0, 2.0, 5.5),
    (2000.0, 3.0, 4.5),
    (3000.0, 3.5, 2.5),
    (4000.0, 5.5, 9.5),
    (6000.0, 2.0, 17.0),
    (8000.0, 0.0, 17.5),
];

impl TestCalibration {
    pub fn nominal(transducer: Transducer) -> Self {
        Self { transducer, full_scale_spl: 100.0, kind: CalibrationKind::Nominal }
    }

    /// RETSPL at a standard audiogram frequency. Panics for any other frequency.
    pub fn retspl(t: Transducer, frequency_hz: f64) -> f64 {
        let row = RETSPL
            .iter()
            .find(|r| r.0 == frequency_hz)
            .unwrap_or_else(|| panic!("No RETSPL for {frequency_hz} Hz"));
        match t {
            Transducer::InEar => row.1,
            Transducer::OverEar => row.2,
        }
    }

    /// Maximum test tone level, never exceeded.
    pub fn cap_dbfs(frequency_hz: f64) -> f64 {
        if frequency_hz < 600.0 {
            -1.0
        } else if frequency_hz < 1600.0 {
            -3.0
        } else {
            -5.0
        }
    }

    pub fn hl_from_dbfs(&self, dbfs: f64, f: f64) -> f64 {
        dbfs + self.full_scale_spl - Self::retspl(self.transducer, f)
    }

    pub fn dbfs_from_hl(&self, hl: f64, f: f64) -> f64 {
        hl - self.full_scale_spl + Self::retspl(self.transducer, f)
    }

    /// Highest HL (multiple of 5) reachable without exceeding the dBFS cap.
    pub fn max_hl(&self, f: f64) -> f64 {
        (self.hl_from_dbfs(Self::cap_dbfs(f), f) / 5.0).floor() * 5.0
    }

    pub fn rounded_hl(hl: f64) -> f64 {
        ((hl / 5.0).round() * 5.0).clamp(-10.0, 120.0)
    }

    pub fn default_transducer(device_name: &str) -> Transducer {
        if device_name.to_lowercase().contains("airpods") { Transducer::InEar } else { Transducer::OverEar }
    }
}
