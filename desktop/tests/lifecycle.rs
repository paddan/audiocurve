//! The app lifecycle rules (Swift `AppDelegate`), the menu bar icon state (Swift `MenuBarIcon`)
//! and, gated, that SIGTERM tears a running route down.

use std::time::Duration;

use audiocurve_app::lifecycle::{
    CloseAction, dock_icon, next_repaint, on_close_request, should_reopen, starts_visible,
};
use audiocurve_app::model::menu::tray_icon_for;
use audiocurve_app::model::settings::AppMode;
use audiocurve_app::platform::TrayIconState;
use audiocurve_core::route::{RouteFailure, RouteTarget};

const BACKGROUND: AppMode = AppMode { background: true };
const FOREGROUND: AppMode = AppMode { background: false };

#[test]
fn closing_the_window_hides_in_background_mode_and_quits_in_foreground_mode() {
    assert_eq!(on_close_request(BACKGROUND), CloseAction::Hide);
    assert_eq!(on_close_request(FOREGROUND), CloseAction::Quit);
}

#[test]
fn a_login_item_launch_starts_hidden() {
    assert!(starts_visible(false));
    assert!(!starts_visible(true));
}

#[test]
fn reopens_only_hidden_on_activation_after_launch_settled() {
    assert!(should_reopen(false, true, true));
    assert!(!should_reopen(true, true, true), "already visible");
    assert!(
        !should_reopen(false, false, true),
        "did not just become active"
    );
    assert!(
        !should_reopen(false, true, false),
        "the launch's own activation"
    );
}

#[test]
fn dock_icon_follows_the_mode_and_the_window() {
    assert!(dock_icon(FOREGROUND, false));
    assert!(dock_icon(FOREGROUND, true));
    assert!(dock_icon(BACKGROUND, true));
    assert!(!dock_icon(BACKGROUND, false));
}

#[test]
fn repaints_at_the_deadline_and_at_least_every_second() {
    let ms = Duration::from_millis;
    assert_eq!(next_repaint(Some(ms(10)), true, false), ms(10));
    assert_eq!(next_repaint(Some(ms(5000)), true, false), ms(1000));
    assert_eq!(next_repaint(None, true, false), ms(1000));
    // Meters showing: 20 fps.
    assert_eq!(next_repaint(Some(ms(5000)), true, true), ms(50));
    // Hidden: the meters do not count, but the tick polls for a reopen.
    assert_eq!(next_repaint(Some(ms(5000)), false, true), ms(250));
    assert_eq!(next_repaint(Some(ms(10)), false, true), ms(10));
}

#[test]
fn tray_icon_follows_the_route_as_menu_bar_icon_does() {
    let uid = || "airpods".to_owned();
    let running = RouteTarget::Running { device_uid: uid() };
    let paused = RouteTarget::Paused { device_uid: uid() };
    assert_eq!(tray_icon_for(&running, true, false), TrayIconState::Active);
    assert_eq!(tray_icon_for(&paused, true, false), TrayIconState::Inactive);
    assert_eq!(
        tray_icon_for(&RouteTarget::Starting, true, false),
        TrayIconState::Inactive
    );
    assert_eq!(
        tray_icon_for(&RouteTarget::Off, false, false),
        TrayIconState::Problem
    );
    for f in [
        RouteFailure::Permission,
        RouteFailure::Plugin,
        RouteFailure::Route,
    ] {
        assert_eq!(
            tray_icon_for(&RouteTarget::Failed(f), true, false),
            TrayIconState::Problem
        );
    }
}

#[test]
fn tray_icon_is_inactive_while_a_test_owns_the_output() {
    // A test on an output that cannot be read leaves the route off, but nothing is wrong.
    assert_eq!(
        tray_icon_for(&RouteTarget::Off, true, true),
        TrayIconState::Inactive
    );
    assert_eq!(
        tray_icon_for(
            &RouteTarget::Paused {
                device_uid: "x".into()
            },
            true,
            true
        ),
        TrayIconState::Inactive
    );
    // Off is off, test or not.
    assert_eq!(
        tray_icon_for(&RouteTarget::Off, false, true),
        TrayIconState::Problem
    );
}

#[cfg(target_os = "macos")]
fn hw_with_output() -> bool {
    use audiocurve_app::platform::macos::hal;
    if std::env::var_os("AUDIOCURVE_HW_TESTS").is_none_or(|v| v != "1") {
        return false;
    }
    if hal::default_output().is_none() {
        eprintln!("no default output; skipped");
        return false;
    }
    true
}

/// The built app, in its test mode, builds an unmuted route on the default output and waits;
/// SIGTERM must tear it down before the process exits with success.
#[cfg(target_os = "macos")]
#[test]
fn sigterm_child_leaves_no_route() {
    use std::io::{BufRead, BufReader};
    use std::process::{Command, Stdio};

    use audiocurve_app::platform::macos::hal;

    if !hw_with_output() {
        return;
    }
    let mut child = Command::new(env!("CARGO_BIN_EXE_audiocurve"))
        .env("AUDIOCURVE_TEST_ROUTE_THEN_WAIT", "1")
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    let ready = lines.next().unwrap().unwrap();
    assert_eq!(ready, "route ready: own routes 1");

    std::thread::sleep(Duration::from_millis(500));
    let killed = Command::new("kill")
        .args(["-TERM", &child.id().to_string()])
        .status()
        .unwrap();
    assert!(killed.success());
    let status = child.wait().unwrap();

    let rest: Vec<String> = lines.map(Result::unwrap).collect();
    assert_eq!(rest, ["torn down: own routes 0"]);
    assert!(status.success(), "{status:?}");
    // The child's aggregate is private to it; this checks no public one leaked either.
    let own = hal::devices()
        .into_iter()
        .filter_map(hal::device_uid)
        .filter(|uid| hal::is_own_route(uid))
        .count();
    assert_eq!(own, 0);
}

/// A panic outside an IO block tears the live route down through the route guard before the
/// process aborts.
#[cfg(target_os = "macos")]
#[test]
fn a_panic_tears_the_route_down_before_aborting() {
    use std::os::unix::process::ExitStatusExt;
    use std::process::Command;

    if !hw_with_output() {
        return;
    }
    let out = Command::new(env!("CARGO_BIN_EXE_audiocurve"))
        .env("AUDIOCURVE_TEST_ROUTE_THEN_WAIT", "panic")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(
        stdout.lines().collect::<Vec<_>>(),
        ["route ready: own routes 1", "panicked: own routes 0"]
    );
    assert_eq!(out.status.signal(), Some(6), "aborted: {:?}", out.status);
}

/// With the UI thread hung, a second SIGTERM ends the process the default way, but tears the
/// live route down first, so the Mac is not left muted.
#[cfg(target_os = "macos")]
#[test]
fn a_second_signal_tears_the_route_down_while_the_main_thread_hangs() {
    use std::io::{BufRead, BufReader};
    use std::os::unix::process::ExitStatusExt;
    use std::process::{Command, Stdio};

    if !hw_with_output() {
        return;
    }
    let mut child = Command::new(env!("CARGO_BIN_EXE_audiocurve"))
        .env("AUDIOCURVE_TEST_ROUTE_THEN_WAIT", "hang")
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    assert_eq!(lines.next().unwrap().unwrap(), "route ready: own routes 1");

    let term = || {
        let sent = Command::new("kill")
            .args(["-TERM", &child.id().to_string()])
            .status()
            .unwrap();
        assert!(sent.success());
    };
    term();
    std::thread::sleep(Duration::from_millis(100));
    term();
    let status = child.wait().unwrap();

    let rest: Vec<String> = lines.map(Result::unwrap).collect();
    assert_eq!(rest, ["second signal: own routes 0"]);
    assert_eq!(status.signal(), Some(15), "{status:?}");
}
