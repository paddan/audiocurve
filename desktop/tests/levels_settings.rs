mod support;

use audiocurve_app::model::levels::Levels;
use audiocurve_app::model::settings::{AppMode, Settings};
use audiocurve_core::fitting::{self, CompressionMode, ParamsInput};

#[test]
fn background_mode_is_a_menu_bar_app() {
    let m = AppMode { background: true };
    assert!(!m.shows_dock_icon(false));
    assert!(m.shows_dock_icon(true));
    assert!(!m.bypass_at_start());
    assert!(!m.quits_when_window_closes());
    assert!(m.offers_launch_at_login());
}

#[test]
fn foreground_mode_is_an_ordinary_app() {
    let m = AppMode { background: false };
    assert!(m.shows_dock_icon(false));
    assert!(m.shows_dock_icon(true));
    assert!(m.bypass_at_start());
    assert!(m.quits_when_window_closes());
    assert!(!m.offers_launch_at_login());
}

#[test]
fn levels_default_like_the_plugin() {
    let l = Levels::default();
    assert_eq!(l.strength, 50.0);
    assert_eq!(l.tone, 0.0);
    assert_eq!(l.mpo_dbfs, -3.0);
    assert_eq!(l.output_gain_db, 0.0);
    assert_eq!(l.input_gain_db, 0.0);
    assert_eq!(l.mode, CompressionMode::Auto);
    assert!(l.compression);
    assert!(l.limiter);
}

#[test]
fn params_scale_strength_and_tone() {
    let p = support::example_profile();
    let l = Levels {
        strength: 50.0,
        tone: 25.0,
        ..Levels::default()
    };
    let mut input = ParamsInput::new(Some(&p), 0.5, l.mpo_dbfs, l.output_gain_db, l.mode);
    input.tone = 0.25;
    assert_eq!(l.params(Some(&p), true), fitting::params(&input));
}

#[test]
fn level_settings_round_trip() {
    let l = Levels {
        strength: 70.0,
        tone: 30.0,
        mpo_dbfs: -6.0,
        output_gain_db: 4.0,
        input_gain_db: -5.0,
        ..Levels::default()
    };
    let mut back = Levels::default();
    back.apply_settings(&l.to_settings());
    assert_eq!(back, l);
}

#[test]
fn settings_persist_and_default_on_corrupt_file() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(Settings::load(dir.path()), Settings::default());
    let s = Settings {
        enabled: false,
        background_processing: false,
        show_in_menu_bar: true,
        asked_launch_at_login: true,
    };
    s.save(dir.path());
    assert_eq!(Settings::load(dir.path()), s);
    std::fs::write(dir.path().join("settings.json"), b"not json").unwrap();
    assert_eq!(Settings::load(dir.path()), Settings::default());
    std::fs::write(dir.path().join("settings.json"), br#"{"version":2,"value":{"enabled":false,"background_processing":false,"show_in_menu_bar":false}}"#).unwrap();
    assert_eq!(Settings::load(dir.path()), Settings::default());
}

#[test]
fn levels_persist_and_default_on_missing_or_corrupt_file() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(Levels::load(dir.path()), Levels::default());
    let l = Levels {
        strength: 70.0,
        tone: 20.0,
        mpo_dbfs: -8.0,
        output_gain_db: 3.0,
        input_gain_db: -4.0,
        mode: CompressionMode::Music,
        compression: false,
        limiter: false,
    };
    l.save(dir.path());
    assert_eq!(Levels::load(dir.path()), l);
    for mode in [CompressionMode::Speech, CompressionMode::Auto] {
        let l = Levels { mode, ..l };
        l.save(dir.path());
        assert_eq!(Levels::load(dir.path()), l);
    }
    std::fs::write(dir.path().join("levels.json"), b"not json").unwrap();
    assert_eq!(Levels::load(dir.path()), Levels::default());
    std::fs::write(
        dir.path().join("levels.json"),
        br#"{"version":2,"value":{"strength":70}}"#,
    )
    .unwrap();
    assert_eq!(Levels::load(dir.path()), Levels::default());
    // A missing field takes its default.
    std::fs::write(
        dir.path().join("levels.json"),
        br#"{"version":1,"value":{"strength":70,"mode":"music"}}"#,
    )
    .unwrap();
    assert_eq!(
        Levels::load(dir.path()),
        Levels {
            strength: 70.0,
            mode: CompressionMode::Music,
            ..Levels::default()
        }
    );
}
