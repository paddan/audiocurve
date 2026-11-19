//! The macOS system services and the menu bar mapping. These only read the system, so they
//! run without `AUDIOCURVE_HW_TESTS`. They never create a tray, change the Dock icon or
//! register a login item.
#![cfg(target_os = "macos")]

mod support;

use std::time::Instant;

use audiocurve_app::i18n::tr;
use audiocurve_app::model::app::{AppModel, Launch};
use audiocurve_app::model::menu::{MenuAction, MenuItem, build_menu};
use audiocurve_app::platform::System;
use audiocurve_app::platform::TrayIconState;
use audiocurve_app::platform::fake::fake;
use audiocurve_app::platform::macos::app_menu::{APP_SETTINGS_ID, app_menu_titles};
use audiocurve_app::platform::macos::system::MacSystem;
use audiocurve_app::platform::macos::tray::{action_map, cross_out, flatten, icon_rgba};
use audiocurve_core::store::ProfileStore;
use support::example_profile;

#[test]
fn app_name_of_finder_is_finder() {
    assert_eq!(MacSystem.app_name("com.apple.finder"), "Finder");
}

#[test]
fn unknown_bundle_id_falls_back() {
    assert_eq!(
        MacSystem.app_name("se.lindefors.audiocurve.no-such-app"),
        tr("another app")
    );
}

/// Every item that carries an action, in the menu tree.
fn actions(items: &[MenuItem], out: &mut Vec<MenuAction>) {
    for i in items {
        match i {
            MenuItem::Check { action, .. } | MenuItem::Action { action, .. } => {
                out.push(action.clone())
            }
            MenuItem::Submenu { items, .. } => actions(items, out),
            MenuItem::Text(_) | MenuItem::Header(_) | MenuItem::Separator => {}
        }
    }
}

#[test]
fn flatten_maps_every_action_once() {
    let dir = tempfile::tempdir().unwrap();
    let (p, _c) = fake();
    let t = Instant::now();
    let mut m = AppModel::new(
        p,
        ProfileStore::new(dir.path().to_path_buf()),
        Launch {
            from_login_item: false,
        },
        t,
    );
    m.tick(t);
    m.editor.set_profile(example_profile(), t);
    m.editor.save_as("Example");
    m.editor.set_profile(example_profile(), t);
    m.editor.save_as("Second");
    m.editor.source_id = None;
    let menu = build_menu(&m);

    let flat = flatten(&menu);
    let mut expected = Vec::new();
    actions(&menu, &mut expected);
    let mapped: Vec<MenuAction> = flat.iter().filter_map(|(_, a)| a.clone()).collect();
    assert_eq!(mapped, expected);
    // On, unsaved + 2 profiles, 3 outputs, 3 × (none + 2 profiles), open, settings, quit.
    assert_eq!(mapped.len(), 19);
    for (i, a) in mapped.iter().enumerate() {
        assert!(!mapped[..i].contains(a), "{a:?} is mapped twice");
    }
    // Paths are the menu ids, so they must be unique, even for items with the same title.
    let paths: Vec<&String> = flat.iter().map(|(p, _)| p).collect();
    for (i, p) in paths.iter().enumerate() {
        assert!(!paths[..i].contains(p), "{p} is not unique");
    }
    assert!(
        flat.iter()
            .any(|(p, a)| p.contains("Profile for AirPods Pro")
                && p.contains("Example")
                && matches!(a, Some(MenuAction::AssignProfile { .. })))
    );
}

#[test]
fn each_icon_is_a_non_empty_square_image_and_they_differ() {
    let states = [
        TrayIconState::Active,
        TrayIconState::Inactive,
        TrayIconState::Problem,
    ];
    let icons: Vec<_> = states.iter().map(|&s| icon_rgba(s)).collect();
    for (state, (rgba, w, h)) in states.iter().zip(&icons) {
        assert_eq!(w, h, "{state:?}");
        assert_eq!(rgba.len(), (w * h * 4) as usize);
        assert!(rgba.chunks(4).any(|px| px[3] > 0), "{state:?} is blank");
        assert!(
            rgba.chunks(4).any(|px| px[3] == 0),
            "{state:?} has no background"
        );
    }
    assert_ne!(icons[0].0, icons[1].0, "ear.fill and ear look the same");
    assert_ne!(icons[1].0, icons[2].0, "the crossed-out ear has no slash");
}

#[test]
fn cross_out_draws_a_slash_from_top_left_to_bottom_right_with_a_gap() {
    // 10 x 10 points at 1x, fully opaque.
    let px = 10;
    let mut rgba = vec![255u8; px * px * 4];
    cross_out(&mut rgba, px, 1.0);
    let alpha = |x: usize, y: usize| rgba[(y * px + x) * 4 + 3];
    // On the slash (top-left to bottom-right, as in Swift's non-flipped drawing).
    assert_eq!(alpha(5, 5), 255);
    assert_eq!(alpha(2, 2), 255);
    // The gap beside it is cleared.
    assert!(alpha(6, 4) < 255, "{}", alpha(6, 4));
    // Far from it the image is untouched.
    assert_eq!(alpha(9, 0), 255);
    assert_eq!(alpha(0, 9), 255);
}

#[test]
fn the_app_menu_settings_item_opens_settings_whatever_the_tray_menu() {
    let map = action_map(&[]);
    assert_eq!(map.get(APP_SETTINGS_ID), Some(&MenuAction::OpenSettings));
    let (platform, _c) = fake();
    let dir = tempfile::tempdir().unwrap();
    let m = AppModel::new(
        platform,
        ProfileStore::new(dir.path().to_path_buf()),
        Launch {
            from_login_item: false,
        },
        Instant::now(),
    );
    let map = action_map(&build_menu(&m));
    assert_eq!(map.get(APP_SETTINGS_ID), Some(&MenuAction::OpenSettings));
}

#[test]
fn the_app_menu_has_settings_and_quit_in_every_language() {
    for lang in [
        audiocurve_app::i18n::Lang::En,
        audiocurve_app::i18n::Lang::Sv,
        audiocurve_app::i18n::Lang::De,
    ] {
        audiocurve_app::i18n::set_lang(lang);
        let titles = app_menu_titles();
        assert!(titles.contains(&tr("Settings…")), "{titles:?}");
        assert!(titles.contains(&tr("Quit AudioCurve")), "{titles:?}");
        assert!(titles.contains(&tr("Hide AudioCurve")), "{titles:?}");
    }
    audiocurve_app::i18n::set_lang(audiocurve_app::i18n::Lang::En);
    assert_ne!(
        {
            audiocurve_app::i18n::set_lang(audiocurve_app::i18n::Lang::Sv);
            tr("Hide AudioCurve")
        },
        "Hide AudioCurve"
    );
    audiocurve_app::i18n::set_lang(audiocurve_app::i18n::Lang::En);
}
