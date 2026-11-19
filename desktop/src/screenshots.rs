//! The egui app's screenshots: the main window with the example audiogram on the fake
//! platform, in English, light and dark, drawn with `egui_kittest` at 2×. Only with the
//! `screenshots` feature (the `screenshots` example and its test turn it on).

use std::path::Path;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use audiocurve_core::hearing_test::TestCalibration;
use audiocurve_core::profile::{Audiogram, AudiogramPoint, HearingProfile};
use audiocurve_core::store::ProfileStore;
use egui::{LayerId, Vec2};
use egui_kittest::Harness;

use crate::i18n::{self, Lang};
use crate::model::app::{AppModel, Launch};
use crate::model::editor::Pane;
use crate::model::hearing::Phase;
use crate::platform::fake::{FakeControl, fake};
use crate::ui::{main_window, theme};

/// The window's default size; at 2× the images are 1800 px wide.
const SIZE: Vec2 = Vec2::new(900.0, 720.0);
const SCALE: f32 = 2.0;
/// The simulated listener hears every tone at or above this level.
const LISTENER_THRESHOLD_HL: f64 = 35.0;

/// Fictional, steeply sloping example audiogram (not a real person's), as in the tests.
const EXAMPLE_LEFT: [(f64, f64); 11] = [
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
const EXAMPLE_RIGHT: [(f64, f64); 11] = [
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

/// Writes `<screen>-light.png` and `<screen>-dark.png` into `dir` (created if missing) for the
/// audiogram, gain, spectrum, hearing-test-running and hearing-test-result screens.
pub fn render_all(dir: &Path) {
    std::fs::create_dir_all(dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display()));
    i18n::set_lang(Lang::En);
    let screens: [(&str, Setup); 5] = [
        ("audiogram", |s| s.m.editor.pane = Pane::Audiogram),
        ("gain", |s| s.m.editor.pane = Pane::Gain),
        ("spectrum", |s| s.m.editor.pane = Pane::Spectrum),
        ("hearing-test-running", Shot::hearing_test_running),
        ("hearing-test-result", Shot::hearing_test_result),
    ];
    for (name, setup) in screens {
        for (theme, suffix) in [(egui::Theme::Light, "light"), (egui::Theme::Dark, "dark")] {
            let mut shot = Shot::new();
            setup(&mut shot);
            let path = dir.join(format!("{name}-{suffix}.png"));
            shot.render(theme, &path);
        }
    }
}

/// The hearing test's date in the screenshots (the result's default profile name shows it).
fn test_date() -> SystemTime {
    // 2026-01-15 12:00 UTC: the same calendar day in every time zone from UTC−11 to UTC+11.
    UNIX_EPOCH + Duration::from_secs(1_768_478_400)
}

/// Puts a fresh `Shot` into the state of one screen.
type Setup = fn(&mut Shot);

/// One screen's model on the fake platform, over a temporary profile store.
struct Shot {
    m: AppModel,
    control: FakeControl,
    t: Instant,
    _dir: tempfile::TempDir,
}

impl Shot {
    /// The example profile with the route running and noise pumped through it, so the gain
    /// and spectrum panes and the meters have content.
    fn new() -> Shot {
        let dir = tempfile::tempdir().expect("temp dir");
        let store = ProfileStore::new(dir.path().to_path_buf());
        let (platform, control) = fake();
        let mut t = Instant::now();
        let launch = Launch {
            from_login_item: false,
        };
        let mut m = AppModel::new(platform, store, launch, t);
        m.editor.set_profile(example_profile(), t);
        settle(&mut m, &mut t);
        for _ in 0..20 {
            control.pump(2400);
        }
        m.tick(t);
        Shot {
            m,
            control,
            t,
            _dir: dir,
        }
    }

    /// The test started and the first tone playing.
    fn hearing_test_running(&mut self) {
        self.open_hearing_test();
        self.m.hearing_start(self.t);
        for _ in 0..100 {
            if self
                .control
                .tone_log()
                .iter()
                .any(|e| e.starts_with("play:"))
            {
                return;
            }
            self.advance();
        }
        panic!("no tone played");
    }

    /// The test run to the end by a listener who hears everything at or above 35 dB HL.
    fn hearing_test_result(&mut self) {
        self.open_hearing_test();
        self.m.hearing_start(self.t);
        let mut seen = self.control.tone_log().len();
        for _ in 0..100_000 {
            if matches!(self.m.hearing().map(|h| h.phase()), Some(Phase::Done(_))) {
                return;
            }
            self.advance();
            let log = self.control.tone_log();
            let heard = log[seen..].iter().any(|e| self.hears(e));
            seen = log.len();
            if heard {
                self.m.hearing_respond(self.t);
            }
        }
        panic!("the hearing test did not finish");
    }

    /// Opens the test with a fixed random source and a fixed date, so every run is the same.
    fn open_hearing_test(&mut self) {
        self.m
            .open_hearing_test_with(Box::new(|| 0.5), Some(test_date()), self.t);
    }

    /// Whether the listener hears a `play:<ear>:<hz>:<dbfs>` tone log entry.
    fn hears(&mut self, entry: &str) -> bool {
        let Some(rest) = entry.strip_prefix("play:") else {
            return false;
        };
        let parts: Vec<&str> = rest.split(':').collect();
        let (Some(Ok(hz)), Some(Ok(dbfs))) = (
            parts.get(1).map(|p| p.parse::<f64>()),
            parts.get(2).map(|p| p.parse::<f64>()),
        ) else {
            return false;
        };
        let Some(h) = self.m.hearing() else {
            return false;
        };
        let cal = TestCalibration::nominal(h.transducer());
        cal.hl_from_dbfs(dbfs, hz) >= LISTENER_THRESHOLD_HL - 1e-6
    }

    /// Ticks at the model's next deadline.
    fn advance(&mut self) {
        let next = self.m.next_deadline().expect("a deadline");
        self.t = next.max(self.t);
        self.m.tick(self.t);
    }

    fn render(self, theme: egui::Theme, path: &Path) {
        let mut h = Harness::builder()
            .with_size(SIZE)
            .with_pixels_per_point(SCALE)
            .with_theme(theme)
            .with_context_setup(theme::apply)
            .build_ui_state(
                |ui, s: &mut Shot| {
                    // Like the app's central panel: the panel colour to the window's edges
                    // (the harness's frame only covers the contents, inset by 8 points).
                    let ctx = ui.ctx().clone();
                    let fill = ui.visuals().panel_fill;
                    ctx.layer_painter(LayerId::background()).rect_filled(
                        ctx.content_rect(),
                        0.0,
                        fill,
                    );
                    main_window::show(ui, &mut s.m, s.t)
                },
                self,
            );
        h.run();
        let image = h.render().expect("render");
        image
            .save(path)
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    }
}

/// Ticks at each deadline for up to 5 s, so the route gets built and runs.
fn settle(m: &mut AppModel, t: &mut Instant) {
    let end = *t + Duration::from_secs(5);
    m.tick(*t);
    while let Some(d) = m.next_deadline().filter(|d| *d <= end) {
        *t = d.max(*t);
        m.tick(*t);
    }
}

fn audiogram_from(points: &[(f64, f64)]) -> Audiogram {
    Audiogram {
        points: points
            .iter()
            .map(|&(f, t)| AudiogramPoint {
                frequency_hz: f,
                threshold_db_hl: t,
            })
            .collect(),
    }
}

fn example_profile() -> HearingProfile {
    HearingProfile {
        left: audiogram_from(&EXAMPLE_LEFT),
        right: audiogram_from(&EXAMPLE_RIGHT),
        left_trim_db: vec![0.0; 12],
        right_trim_db: vec![0.0; 12],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The result is saved under the fixed date, not today's, so the images are the same on
    /// every day they are drawn.
    #[test]
    fn the_hearing_test_result_carries_the_fixed_date() {
        let mut shot = Shot::new();
        shot.hearing_test_result();
        let h = shot.m.hearing().expect("a hearing session");
        assert_eq!(i18n::formatted_date(h.test_date(), Lang::En), "15 Jan 2026");
        assert_eq!(
            h.default_name(&i18n::formatted_date(h.test_date(), Lang::En)),
            "AirPods Pro – 15 Jan 2026"
        );
    }
}
