//! No public API tells whether audio capture was denied: a denied tap delivers silence (Swift
//! `AppModel.checkPermission`).

use std::time::{Duration, Instant};

/// How long the input must stay silent, while other apps play, to count as denied.
pub const SILENCE_FOR_DENIED: Duration = Duration::from_secs(3);

/// Denied only when the route's input has never carried sound (`since_signal` is `None`: once it
/// has, capture works and a later pause means nothing), others play, and the route has delivered
/// buffers for `SILENCE_FOR_DENIED`, so an output that starts slowly (Bluetooth) never counts.
pub fn looks_denied(
    since_signal: Option<Duration>,
    others_playing: bool,
    since_first_buffer: Duration,
) -> bool {
    since_signal.is_none() && others_playing && since_first_buffer >= SILENCE_FOR_DENIED
}

/// How long a started route may go without an IO callback before it counts as failed.
pub const START_TIMEOUT: Duration = Duration::from_secs(5);

/// A route that has had no IO callback `START_TIMEOUT` after starting: the tap mutes the Mac but
/// nothing plays, so it must be reported failed (and, if that repeats, torn down).
pub fn start_stalled(since_start: Duration, callbacks: u64) -> bool {
    callbacks == 0 && since_start >= START_TIMEOUT
}

/// A route whose IO called back and then stopped for `START_TIMEOUT`: as muted and silent as
/// one that never started. `since_last_callback` is in host time, which stands still in deep
/// sleep but not in dark wake (Power Nap), so `Watchdog` gates this around sleep and wake.
pub fn stalled_mid_run(callbacks: u64, since_last_callback: Option<Duration>) -> bool {
    callbacks > 0 && since_last_callback.is_some_and(|d| d >= START_TIMEOUT)
}

/// After a full wake the watchdog waits this long before it judges a route again: the HAL may
/// take a while to restart IO after the Mac woke.
pub const WAKE_GRACE: Duration = Duration::from_secs(10);

/// A suspension that no did-wake ends (a missed notification) lapses after this much awake
/// time, so the watchdog never stays off for good. Dark wake and Power Nap count here; deep
/// sleep does not (`Instant` stands still in it).
pub const MAX_SUSPEND: Duration = Duration::from_secs(10 * 60);

/// This many mid-run stalls within `STALL_WINDOW` (no wake in between) count as a failed render,
/// so IO that keeps stopping ends in the death latch and a teardown, not an endless rebuild loop
/// with the Mac muted.
pub const STALL_LIMIT: usize = 3;
pub const STALL_WINDOW: Duration = Duration::from_secs(60);

/// What a stall check reports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StallReport {
    /// Rebuild the route; it does not count toward the death latch.
    Rebuild,
    /// Report a failed render (`RenderFailed`).
    Failed,
}

/// The stall watchdog's view of system sleep, and the recent mid-run stalls. Pure: the caller
/// passes the time and the sleep/wake notifications.
#[derive(Debug, Default)]
pub struct Watchdog {
    /// Will-sleep seen and no did-wake yet.
    asleep_since: Option<Instant>,
    woke_at: Option<Instant>,
    stalls: Vec<Instant>,
}

impl Watchdog {
    /// `NSWorkspaceWillSleepNotification`: the watchdog stops judging routes.
    pub fn will_sleep(&mut self, now: Instant) {
        self.asleep_since.get_or_insert(now);
    }

    /// `NSWorkspaceDidWakeNotification`: the watchdog judges routes again after `WAKE_GRACE`.
    /// Stalls before the sleep no longer count.
    pub fn did_wake(&mut self, now: Instant) {
        self.asleep_since = None;
        self.woke_at = Some(now);
        self.stalls.clear();
    }

    /// Whether a route may be judged stalled at `now`: not while the Mac sleeps (or is in dark
    /// wake), and not within `WAKE_GRACE` of a wake.
    pub fn watching(&self, now: Instant) -> bool {
        let asleep = self
            .asleep_since
            .is_some_and(|t| now.saturating_duration_since(t) < MAX_SUSPEND);
        let in_grace = self
            .woke_at
            .is_some_and(|t| now.saturating_duration_since(t) < WAKE_GRACE);
        !asleep && !in_grace
    }

    /// Records a mid-run stall at `now`: a rebuild, unless it is the `STALL_LIMIT`th within
    /// `STALL_WINDOW` (or later in that window).
    pub fn mid_run_stall(&mut self, now: Instant) -> StallReport {
        self.stalls
            .retain(|&t| now.saturating_duration_since(t) < STALL_WINDOW);
        self.stalls.push(now);
        // Not cleared once it fails: the next stall in the window fails too, so a second death
        // follows within the controller's `DEATH_WINDOW` and latches.
        if self.stalls.len() >= STALL_LIMIT {
            StallReport::Failed
        } else {
            StallReport::Rebuild
        }
    }
}
