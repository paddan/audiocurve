//! The system-wide route. The pure tests always run; the hardware tests build real routes on the
//! default output, unmuted, and only with `AUDIOCURVE_HW_TESTS=1`.
#![cfg(target_os = "macos")]

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use audiocurve_app::engine::{Engine, SharedEngine};
use audiocurve_app::platform::Audio;
use audiocurve_app::platform::PlatformEvent;
use audiocurve_app::platform::macos::audio::EventQueue;
use audiocurve_app::platform::macos::audio::MacAudio;
use audiocurve_app::platform::macos::capture_check::{
    MAX_SUSPEND, STALL_LIMIT, STALL_WINDOW, StallReport, WAKE_GRACE, Watchdog, looks_denied,
    stalled_mid_run, start_stalled,
};
use audiocurve_app::platform::macos::hal;
use audiocurve_app::platform::macos::route::RouteSignal;
use audiocurve_app::platform::macos::route::{MacRoute, MacRouteBuilder};
use audiocurve_core::route::{BuiltRoute, RouteBuilder};
use audiocurve_dsp::Params;

const S: fn(f64) -> Duration = Duration::from_secs_f64;

#[test]
fn looks_denied_needs_others_playing_and_3s_of_silence() {
    assert!(looks_denied(None, true, S(3.0)));
    assert!(looks_denied(None, true, S(10.0)));
    assert!(!looks_denied(None, false, S(10.0)));
    assert!(!looks_denied(None, true, S(2.9)));
}

#[test]
fn slow_bluetooth_start_is_not_denied() {
    assert!(!looks_denied(None, true, S(1.5)));
}

/// Swift `checkPermission`: once the input has carried sound, capture works; a later pause
/// (music stopped while a browser keeps its output running) is never taken as denied.
#[test]
fn a_route_that_carried_signal_is_never_denied() {
    assert!(!looks_denied(Some(S(10.0)), true, S(60.0)));
    assert!(!looks_denied(Some(S(3.0)), true, S(3.0)));
    assert!(!looks_denied(Some(S(0.1)), true, S(60.0)));
}

/// The first sound on a route reports `CaptureWorks`, once.
#[test]
fn the_first_signal_reports_capture_works_once() {
    let mut audio = MacAudio::new(Arc::new(|| {}));
    let id = uuid::Uuid::new_v4();
    let signal = Arc::new(RouteSignal::running(id, S(0.01), Some(S(0.1))));
    audio.set_running_route(&signal);
    assert_eq!(audio.poll_events(), vec![PlatformEvent::CaptureWorks]);
    assert!(audio.poll_events().is_empty());
}

#[test]
fn start_stalled_after_5s_without_callbacks() {
    assert!(!start_stalled(S(4.9), 0));
    assert!(start_stalled(S(5.0), 0));
    assert!(start_stalled(S(60.0), 0));
    assert!(!start_stalled(S(5.0), 1));
    assert!(!start_stalled(S(60.0), 1000));
}

#[test]
fn stalled_mid_run_after_5s_since_the_last_callback() {
    assert!(
        !stalled_mid_run(0, None),
        "no callback yet: `start_stalled`'s case"
    );
    assert!(!stalled_mid_run(10, Some(S(0.01))));
    assert!(!stalled_mid_run(10, Some(S(4.9))));
    assert!(stalled_mid_run(10, Some(S(5.0))));
    assert!(stalled_mid_run(1, Some(S(60.0))));
}

/// A route whose IO never called back is reported as failed once, so the model rebuilds it and
/// then gives up, tearing it down (the Mac is never left muted).
#[test]
fn a_route_without_callbacks_for_5s_is_reported_failed_once() {
    let mut audio = MacAudio::new(Arc::new(|| {}));
    let id = uuid::Uuid::new_v4();
    let young = Arc::new(RouteSignal::started_at(id, Instant::now()));
    audio.set_running_route(&young);
    assert!(audio.poll_events().is_empty());

    let stalled = Arc::new(RouteSignal::started_at(id, Instant::now() - S(5.0)));
    audio.set_running_route(&stalled);
    assert_eq!(
        audio.poll_events(),
        vec![PlatformEvent::RenderFailed { route_id: id }]
    );
    assert!(audio.poll_events().is_empty());
    assert!(audio.poll_events().is_empty());
}

/// Stall reports only, not the capture check's (it reads the real HAL).
fn stalls(audio: &mut MacAudio) -> Vec<PlatformEvent> {
    audio
        .poll_events()
        .into_iter()
        .filter(|e| {
            matches!(
                e,
                PlatformEvent::RenderFailed { .. } | PlatformEvent::RouteStalled { .. }
            )
        })
        .collect()
}

/// A route whose IO called back and then stopped keeps the Mac muted with nothing playing: it is
/// reported stalled once, a rebuild that does not count as a failed render (sleep and wake can
/// pause IO like this).
#[test]
fn a_route_whose_callbacks_stopped_for_5s_is_reported_stalled_once() {
    let mut audio = MacAudio::new(Arc::new(|| {}));
    let id = uuid::Uuid::new_v4();
    let live = Arc::new(RouteSignal::running(id, S(0.01), None));
    audio.set_running_route(&live);
    assert!(stalls(&mut audio).is_empty());

    let stopped = Arc::new(RouteSignal::running(id, S(5.0), None));
    audio.set_running_route(&stopped);
    assert_eq!(
        stalls(&mut audio),
        vec![PlatformEvent::RouteStalled { route_id: id }]
    );
    assert!(stalls(&mut audio).is_empty());
}

/// IO that keeps stopping mid-run is reported as a failed render from the `STALL_LIMIT`th stall,
/// so the model's death latch tears the route down rather than rebuilding it forever.
#[test]
fn repeated_mid_run_stalls_become_render_failures() {
    let mut audio = MacAudio::new(Arc::new(|| {}));
    let mut kinds = Vec::new();
    for _ in 0..STALL_LIMIT + 1 {
        let id = uuid::Uuid::new_v4();
        let stopped = Arc::new(RouteSignal::running(id, S(5.0), None));
        audio.set_running_route(&stopped);
        kinds.extend(stalls(&mut audio).into_iter().map(|e| match e {
            PlatformEvent::RouteStalled { .. } => "stalled",
            _ => "failed",
        }));
    }
    let mut want = vec!["stalled"; STALL_LIMIT - 1];
    want.extend(["failed", "failed"]);
    assert_eq!(kinds, want);
}

/// While the Mac sleeps the watchdog judges no route; after a wake it waits `WAKE_GRACE`, and
/// the wake re-baselines the route's last callback, so the sleep itself is not a stall.
#[test]
fn the_grace_period_after_a_wake_suppresses_a_stall() {
    let mut audio = MacAudio::new(Arc::new(|| {}));
    let id = uuid::Uuid::new_v4();
    let stopped = Arc::new(RouteSignal::running(id, S(60.0), None));
    audio.set_running_route(&stopped);

    audio.will_sleep(Instant::now());
    assert!(stalls(&mut audio).is_empty(), "asleep");

    audio.did_wake(Instant::now());
    assert!(stalls(&mut audio).is_empty(), "in the grace period");
    let since = stopped.since_last_callback().unwrap();
    assert!(since < S(1.0), "re-baselined on wake: {since:?}");

    // The grace period is over and IO has not called back since the wake.
    audio.did_wake(Instant::now() - WAKE_GRACE);
    let stopped = Arc::new(RouteSignal::running(id, S(5.0), None));
    audio.set_running_route(&stopped);
    // Re-baselined again by this poll, so not stalled yet.
    assert!(stalls(&mut audio).is_empty());
    let stopped = Arc::new(RouteSignal::running(id, S(5.0), None));
    audio.set_running_route(&stopped);
    assert_eq!(
        stalls(&mut audio),
        vec![PlatformEvent::RouteStalled { route_id: id }]
    );
}

#[test]
fn watchdog_is_off_while_asleep_and_for_the_grace_after_a_wake() {
    let t = Instant::now();
    let mut w = Watchdog::default();
    assert!(w.watching(t));
    w.will_sleep(t);
    assert!(!w.watching(t + S(60.0)), "dark wake");
    w.did_wake(t + S(60.0));
    assert!(!w.watching(t + S(60.0) + WAKE_GRACE - S(0.1)));
    assert!(w.watching(t + S(60.0) + WAKE_GRACE));
}

/// A will-sleep with no did-wake (a missed notification) lapses, so the Mac is never left muted
/// by a stalled route that is no longer watched.
#[test]
fn a_suspension_without_a_wake_lapses() {
    let t = Instant::now();
    let mut w = Watchdog::default();
    w.will_sleep(t);
    assert!(!w.watching(t + MAX_SUSPEND - S(1.0)));
    assert!(w.watching(t + MAX_SUSPEND));
}

#[test]
fn stalls_escalate_within_the_window_and_a_wake_forgets_them() {
    let t = Instant::now();
    let mut w = Watchdog::default();
    let at = |s: f64| t + S(s);
    assert_eq!(w.mid_run_stall(at(0.0)), StallReport::Rebuild);
    assert_eq!(w.mid_run_stall(at(6.0)), StallReport::Rebuild);
    assert_eq!(w.mid_run_stall(at(12.0)), StallReport::Failed);
    assert_eq!(w.mid_run_stall(at(18.0)), StallReport::Failed);

    let mut w = Watchdog::default();
    w.mid_run_stall(at(0.0));
    w.mid_run_stall(at(6.0));
    assert_eq!(
        w.mid_run_stall(at(0.0) + STALL_WINDOW),
        StallReport::Rebuild,
        "the first fell out of the window"
    );

    let mut w = Watchdog::default();
    w.mid_run_stall(at(0.0));
    w.mid_run_stall(at(6.0));
    w.did_wake(at(7.0));
    assert_eq!(w.mid_run_stall(at(20.0)), StallReport::Rebuild);
}

#[test]
fn engine_with_sample_rate_keeps_params() {
    let e = Engine::new(48_000.0, 4096);
    assert_eq!(e.sample_rate(), 48_000.0);
    assert_eq!(e.last_params(), Params::unity());
    let mut p = Params::unity();
    p.output_gain_db = -6.0;
    p.speech_weight = 0.25;
    e.set_params(&p);
    assert_eq!(e.last_params(), p);
    let f = e.with_sample_rate(44_100.0);
    assert_eq!(f.sample_rate(), 44_100.0);
    assert_eq!(f.max_frames(), 4096);
    assert_eq!(f.last_params(), p);
}

// --- Hardware ---

/// The hardware tests build one route at a time.
static HW: Mutex<()> = Mutex::new(());

fn hw() -> Option<MutexGuard<'static, ()>> {
    if std::env::var("AUDIOCURVE_HW_TESTS").as_deref() != Ok("1") {
        return None;
    }
    Some(HW.lock().unwrap_or_else(|e| e.into_inner()))
}

fn engine(rate: f64) -> SharedEngine {
    Rc::new(RefCell::new(Arc::new(Engine::new(rate, 4096))))
}

fn builder(engine: SharedEngine) -> (MacRouteBuilder, EventQueue) {
    let events = EventQueue::new(Arc::new(|| {}));
    (MacRouteBuilder::new(engine, events.clone(), false), events)
}

fn default_uid() -> String {
    hal::device_uid(hal::default_output().expect("a default output")).expect("a UID")
}

fn own_routes() -> Vec<String> {
    hal::devices()
        .into_iter()
        .filter_map(hal::device_uid)
        .filter(|uid| hal::is_own_route(uid))
        .collect()
}

/// The HAL updates its device list shortly after a destroy, so this waits up to 1 s.
fn own_routes_after_teardown() -> Vec<String> {
    let end = Instant::now() + Duration::from_secs(1);
    while !own_routes().is_empty() && Instant::now() < end {
        std::thread::sleep(Duration::from_millis(20));
    }
    own_routes()
}

fn is_listed_own_route(aggregate: hal::ObjectId) -> bool {
    let uid = hal::device_uid(aggregate).expect("an aggregate UID");
    hal::is_own_route(&uid) && own_routes().contains(&uid)
}

fn wait_for_callbacks(route: &MacRoute, n: u64, within: Duration) -> u64 {
    let end = Instant::now() + within;
    while route.callbacks() < n && Instant::now() < end {
        std::thread::sleep(Duration::from_millis(20));
    }
    route.callbacks()
}

#[test]
fn route_builds_runs_and_tears_down_on_the_default_output() {
    let Some(_hw) = hw() else { return };
    let (mut b, events) = builder(engine(48_000.0));
    let route = b.build_route(&default_uid()).expect("a route");
    assert!(is_listed_own_route(route.aggregate()));
    let n = wait_for_callbacks(&route, 10, Duration::from_secs(3));
    let failures: Vec<_> = events
        .drain()
        .into_iter()
        .filter(|e| matches!(e, PlatformEvent::RenderFailed { .. }))
        .collect();
    Box::new(route).tear_down();
    assert!(n >= 10, "only {n} IO callbacks in 3 s");
    assert!(failures.is_empty(), "{failures:?}");
    let left = own_routes_after_teardown();
    assert!(left.is_empty(), "{left:?}");
}

#[test]
fn dropping_a_route_tears_it_down() {
    let Some(_hw) = hw() else { return };
    assert!(own_routes_after_teardown().is_empty());
    let (mut b, _events) = builder(engine(48_000.0));
    let route = b.build(&default_uid()).expect("a route");
    assert_eq!(own_routes().len(), 1, "{:?}", own_routes());
    drop(route);
    let left = own_routes_after_teardown();
    assert!(left.is_empty(), "{left:?}");
}

#[test]
fn engine_follows_the_aggregate_rate() {
    let Some(_hw) = hw() else { return };
    // A rate other than the output's, so the build must swap in a new engine.
    let output_rate = hal::nominal_sample_rate(hal::default_output().unwrap()).unwrap();
    let start_rate = if output_rate == 44_100.0 {
        48_000.0
    } else {
        44_100.0
    };
    let shared = engine(start_rate);
    let mut p = Params::unity();
    p.output_gain_db = -3.0;
    shared.borrow().set_params(&p);
    let before = shared.borrow().clone();
    let (mut b, _events) = builder(shared.clone());
    let route = b.build_route(&default_uid()).expect("a route");
    let rate = hal::nominal_sample_rate(route.aggregate()).expect("an aggregate rate");
    let after = shared.borrow().clone();
    drop(route);
    assert_ne!(rate, start_rate, "the swap branch must run");
    assert!(!Arc::ptr_eq(&before, &after), "a new engine was swapped in");
    assert_eq!(after.sample_rate(), rate);
    assert_eq!(after.last_params(), p);
}

#[test]
fn device_events_push_sample_rate_changed() {
    let Some(_hw) = hw() else { return };
    let (mut b, events) = builder(engine(48_000.0));
    let route = b.build_route(&default_uid()).expect("a route");
    events.drain();
    route.simulate_device_event();
    let queued = events.drain();
    drop(route);
    assert_eq!(queued, vec![PlatformEvent::SampleRateChanged]);
}
