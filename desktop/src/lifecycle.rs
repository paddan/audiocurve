//! The app lifecycle rules (Swift `AppDelegate`): what closing the window does, whether the
//! app starts visible, when it reopens, the Dock icon and how often it ticks.

use std::time::Duration;

use crate::model::settings::AppMode;

/// After launch, the app's own activation is not a reopen.
pub const LAUNCH_SETTLE: Duration = Duration::from_secs(1);
/// The meters are redrawn at least this often while the window shows them.
pub const METER_INTERVAL: Duration = Duration::from_millis(50);
/// While hidden the app still ticks this often: the route watchdogs and the reopen check.
pub const HIDDEN_TICK: Duration = Duration::from_millis(250);
/// The model ticks at least this often.
pub const MAX_TICK: Duration = Duration::from_secs(1);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CloseAction {
    Hide,
    Quit,
}

/// The main window's close button: the background app keeps running, an ordinary app quits.
pub fn on_close_request(mode: AppMode) -> CloseAction {
    if mode.quits_when_window_closes() {
        CloseAction::Quit
    } else {
        CloseAction::Hide
    }
}

/// Launched by the login item, the app starts without its window.
pub fn starts_visible(from_login_item: bool) -> bool {
    !from_login_item
}

/// A Dock click or a double-click in Finder while hidden (Swift
/// `applicationShouldHandleReopen`), seen as the app becoming active once launch has settled.
pub fn should_reopen(window_visible: bool, became_active: bool, launch_settled: bool) -> bool {
    !window_visible && became_active && launch_settled
}

pub fn dock_icon(mode: AppMode, window_open: bool) -> bool {
    mode.shows_dock_icon(window_open)
}

/// How long until the next tick: the model's next deadline, at most `MAX_TICK`; while visible
/// with meters, at most `METER_INTERVAL`; while hidden, at most `HIDDEN_TICK`.
pub fn next_repaint(
    until_deadline: Option<Duration>,
    window_visible: bool,
    meters: bool,
) -> Duration {
    let cap = match (window_visible, meters) {
        (false, _) => HIDDEN_TICK,
        (true, true) => METER_INTERVAL,
        (true, false) => MAX_TICK,
    };
    until_deadline.map_or(cap, |d| d.min(cap))
}
