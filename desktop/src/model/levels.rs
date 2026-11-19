//! The level controls of the main window, with the plugin's defaults and slider ranges. They
//! are kept between launches in `levels.json` in the store root (Swift keeps them in the
//! plugin's `fullState`).

use std::fs;
use std::ops::RangeInclusive;
use std::path::Path;

use audiocurve_core::fitting::{self, CompressionMode, ParamsInput};
use audiocurve_core::outputs::LevelSettings;
use audiocurve_core::profile::HearingProfile;
use audiocurve_dsp::Params;
use serde_json::{Value, json};

const FILE_NAME: &str = "levels.json";
const FILE_VERSION: u64 = 1;

/// Slider ranges, from `AU/Parameters.swift`.
pub const MPO_RANGE: RangeInclusive<f64> = -20.0..=-1.0;
pub const OUTPUT_GAIN_RANGE: RangeInclusive<f64> = -48.0..=12.0;
pub const INPUT_GAIN_RANGE: RangeInclusive<f64> = -30.0..=12.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Levels {
    /// Percent, 0...100.
    pub strength: f64,
    /// Percent, 0...100.
    pub tone: f64,
    pub mpo_dbfs: f64,
    pub output_gain_db: f64,
    pub input_gain_db: f64,
    pub mode: CompressionMode,
    pub compression: bool,
    pub limiter: bool,
}

impl Default for Levels {
    fn default() -> Self {
        Levels {
            strength: 50.0,
            tone: 0.0,
            mpo_dbfs: -3.0,
            output_gain_db: 0.0,
            input_gain_db: 0.0,
            mode: CompressionMode::Auto,
            compression: true,
            limiter: true,
        }
    }
}

impl Levels {
    /// The kernel parameters; strength and tone are percentages here and fractions there.
    pub fn params(&self, profile: Option<&HearingProfile>, active: bool) -> Params {
        fitting::params(&ParamsInput {
            profile,
            strength: self.strength / 100.0,
            tone: self.tone / 100.0,
            mpo_dbfs: self.mpo_dbfs,
            output_gain_db: self.output_gain_db,
            mode: self.mode,
            compression: self.compression,
            limiter: self.limiter,
            input_gain_db: self.input_gain_db,
            active,
        })
    }

    /// The levels remembered per output. As in the Swift app, strength and tone stay percentages.
    pub fn to_settings(&self) -> LevelSettings {
        LevelSettings {
            strength: self.strength,
            input_gain_db: self.input_gain_db,
            output_gain_db: self.output_gain_db,
            tone: self.tone,
            mpo_dbfs: self.mpo_dbfs,
        }
    }

    pub fn apply_settings(&mut self, l: &LevelSettings) {
        self.strength = l.strength;
        self.input_gain_db = l.input_gain_db;
        self.output_gain_db = l.output_gain_db;
        self.tone = l.tone;
        self.mpo_dbfs = l.mpo_dbfs;
    }
}

fn mode_name(m: CompressionMode) -> &'static str {
    match m {
        CompressionMode::Speech => "speech",
        CompressionMode::Music => "music",
        CompressionMode::Auto => "auto",
    }
}

fn mode_from_name(s: &str) -> Option<CompressionMode> {
    match s {
        "speech" => Some(CompressionMode::Speech),
        "music" => Some(CompressionMode::Music),
        "auto" => Some(CompressionMode::Auto),
        _ => None,
    }
}

impl Levels {
    /// The saved levels; defaults when the file is missing, corrupt or of another version.
    /// A missing or invalid field takes its default.
    pub fn load(root: &Path) -> Levels {
        let parse = || -> Option<Levels> {
            let data = fs::read(root.join(FILE_NAME)).ok()?;
            let env: Value = serde_json::from_slice(&data).ok()?;
            if env.get("version")?.as_u64()? != FILE_VERSION {
                return None;
            }
            let v = env.get("value")?.as_object()?;
            let d = Levels::default();
            let num = |k: &str, default: f64| {
                v.get(k)
                    .and_then(Value::as_f64)
                    .filter(|x| x.is_finite())
                    .unwrap_or(default)
            };
            let flag =
                |k: &str, default: bool| v.get(k).and_then(Value::as_bool).unwrap_or(default);
            Some(Levels {
                strength: num("strength", d.strength).clamp(0.0, 100.0),
                tone: num("tone", d.tone).clamp(0.0, 100.0),
                mpo_dbfs: num("mpo_dbfs", d.mpo_dbfs).clamp(*MPO_RANGE.start(), *MPO_RANGE.end()),
                output_gain_db: num("output_gain_db", d.output_gain_db)
                    .clamp(*OUTPUT_GAIN_RANGE.start(), *OUTPUT_GAIN_RANGE.end()),
                input_gain_db: num("input_gain_db", d.input_gain_db)
                    .clamp(*INPUT_GAIN_RANGE.start(), *INPUT_GAIN_RANGE.end()),
                mode: v
                    .get("mode")
                    .and_then(Value::as_str)
                    .and_then(mode_from_name)
                    .unwrap_or(d.mode),
                compression: flag("compression", d.compression),
                limiter: flag("limiter", d.limiter),
            })
        };
        parse().unwrap_or_default()
    }

    /// Best effort, atomic: a unique temporary file in the same directory, then a rename.
    pub fn save(&self, root: &Path) {
        let _ = self.try_save(root);
    }

    fn try_save(&self, root: &Path) -> std::io::Result<()> {
        fs::create_dir_all(root)?;
        let data = serde_json::to_vec(&json!({
            "version": FILE_VERSION,
            "value": {
                "strength": self.strength,
                "tone": self.tone,
                "mpo_dbfs": self.mpo_dbfs,
                "output_gain_db": self.output_gain_db,
                "input_gain_db": self.input_gain_db,
                "mode": mode_name(self.mode),
                "compression": self.compression,
                "limiter": self.limiter,
            },
        }))?;
        let tmp = root.join(format!(
            ".{FILE_NAME}.{}.{}.tmp",
            std::process::id(),
            uuid::Uuid::new_v4().simple()
        ));
        fs::write(&tmp, data)?;
        fs::rename(&tmp, root.join(FILE_NAME)).inspect_err(|_| {
            let _ = fs::remove_file(&tmp);
        })
    }
}
