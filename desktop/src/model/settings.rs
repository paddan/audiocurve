//! App settings (`settings.json` in the store root) and the app mode derived from them.

use std::fs;
use std::path::Path;

use serde_json::{Value, json};

const FILE_NAME: &str = "settings.json";
const FILE_VERSION: u64 = 1;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    pub enabled: bool,
    pub background_processing: bool,
    pub show_in_menu_bar: bool,
    /// The one-time "Start AudioCurve when you log in?" question was asked.
    pub asked_launch_at_login: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            enabled: true,
            background_processing: true,
            show_in_menu_bar: true,
            asked_launch_at_login: false,
        }
    }
}

impl Settings {
    /// The saved settings; defaults when the file is missing, corrupt or of another version.
    /// A missing field takes its default.
    pub fn load(root: &Path) -> Settings {
        let parse = || -> Option<Settings> {
            let data = fs::read(root.join(FILE_NAME)).ok()?;
            let env: Value = serde_json::from_slice(&data).ok()?;
            if env.get("version")?.as_u64()? != FILE_VERSION {
                return None;
            }
            let v = env.get("value")?.as_object()?;
            let d = Settings::default();
            let flag =
                |k: &str, default: bool| v.get(k).and_then(Value::as_bool).unwrap_or(default);
            Some(Settings {
                enabled: flag("enabled", d.enabled),
                background_processing: flag("background_processing", d.background_processing),
                show_in_menu_bar: flag("show_in_menu_bar", d.show_in_menu_bar),
                asked_launch_at_login: flag("asked_launch_at_login", d.asked_launch_at_login),
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
                "enabled": self.enabled,
                "background_processing": self.background_processing,
                "show_in_menu_bar": self.show_in_menu_bar,
                "asked_launch_at_login": self.asked_launch_at_login,
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

/// How the app behaves: a background menu bar app that processes all system audio, or an
/// ordinary app that processes sound only while its window is open and Bypass is off.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AppMode {
    pub background: bool,
}

impl AppMode {
    /// In background mode the Dock icon shows only while the main window is open.
    pub fn shows_dock_icon(&self, window_open: bool) -> bool {
        !self.background || window_open
    }

    /// Outside background mode the sound plays unprocessed until the user unticks Bypass.
    pub fn bypass_at_start(&self) -> bool {
        !self.background
    }

    /// An ordinary app quits when its window closes.
    pub fn quits_when_window_closes(&self) -> bool {
        !self.background
    }

    /// Launching at login only makes sense for the background app.
    pub fn offers_launch_at_login(&self) -> bool {
        self.background
    }
}
