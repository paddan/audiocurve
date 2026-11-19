//! `Audio` on macOS: outputs from the HAL, device change events, and the route.

use std::sync::{Arc, Mutex};
use std::time::Instant;

use audiocurve_core::outputs::OutputDevice;
use audiocurve_core::route::RouteBuilder;
use audiocurve_core::store::natural_cmp;
use objc2_core_audio::{kAudioHardwarePropertyDefaultOutputDevice, kAudioHardwarePropertyDevices};

use super::capture_check::{StallReport, Watchdog, looks_denied, stalled_mid_run, start_stalled};
use super::diag;
use super::hal::{self, Listener};
use super::power::{self, PowerObservers};
use super::route::{MacRouteBuilder, RouteSignal, RunningRoute};
use crate::engine::SharedEngine;
use crate::platform::{Audio, PlatformEvent};

pub type Waker = Arc<dyn Fn() + Send + Sync>;

/// Platform events queued from any thread; `push` wakes the UI.
#[derive(Clone)]
pub struct EventQueue {
    events: Arc<Mutex<Vec<PlatformEvent>>>,
    waker: Waker,
}

impl EventQueue {
    pub fn new(waker: Waker) -> Self {
        EventQueue {
            events: Arc::default(),
            waker,
        }
    }

    pub fn push(&self, e: PlatformEvent) {
        self.events
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(e);
        (self.waker)();
    }

    pub fn drain(&self) -> Vec<PlatformEvent> {
        std::mem::take(&mut *self.events.lock().unwrap_or_else(|e| e.into_inner()))
    }
}

/// The watchdog and whether a wake still has to re-baseline the running route; written by the
/// sleep/wake observers on whichever thread posts them.
#[derive(Default)]
struct Power {
    watchdog: Watchdog,
    rebaseline: bool,
}

pub struct MacAudio {
    events: EventQueue,
    running: RunningRoute,
    power: Arc<Mutex<Power>>,
    _listeners: Vec<Listener>,
    _power_observers: PowerObservers,
}

impl MacAudio {
    /// `waker` is called after an event is queued, on whichever thread queued it.
    pub fn new(waker: Waker) -> Self {
        let events = EventQueue::new(waker);
        let listeners = [
            kAudioHardwarePropertyDefaultOutputDevice,
            kAudioHardwarePropertyDevices,
        ]
        .into_iter()
        .map(|selector| {
            let events = events.clone();
            hal::listen(hal::SYSTEM, selector, move || {
                events.push(PlatformEvent::OutputsChanged)
            })
        })
        .collect();
        let power: Arc<Mutex<Power>> = Arc::default();
        let observers = {
            let (sleep, wake) = (power.clone(), power.clone());
            let waker = events.waker.clone();
            power::observe_sleep(
                move || lock(&sleep).watchdog.will_sleep(Instant::now()),
                move || {
                    wake_at(&wake, Instant::now());
                    waker();
                },
            )
        };
        MacAudio {
            events,
            running: RunningRoute::default(),
            power,
            _listeners: listeners,
            _power_observers: observers,
        }
    }

    /// Test hook: the Mac went to sleep at `at`.
    #[doc(hidden)]
    pub fn will_sleep(&self, at: Instant) {
        lock(&self.power).watchdog.will_sleep(at);
    }

    /// Test hook: the Mac woke at `at`.
    #[doc(hidden)]
    pub fn did_wake(&self, at: Instant) {
        wake_at(&self.power, at);
    }

    /// Test hook: makes `signal` the running route that `poll_events` checks.
    #[doc(hidden)]
    pub fn set_running_route(&mut self, signal: &Arc<RouteSignal>) {
        *self.running.borrow_mut() = Arc::downgrade(signal);
    }

    /// Once per route, when IO never called back after starting (`RenderFailed`) or stopped
    /// calling back (`RouteStalled`, or `RenderFailed` when that keeps happening): the tap would
    /// keep the Mac muted with nothing playing. The model rebuilds, and on repeated failures
    /// gives up and tears down. Not while the Mac sleeps or just after it woke: IO may pause
    /// then, and the Mac is not heard anyway.
    fn check_stall(&self) -> Option<PlatformEvent> {
        let signal = self.running.borrow().upgrade()?;
        let now = Instant::now();
        let mut power = lock(&self.power);
        if std::mem::take(&mut power.rebaseline) {
            signal.rebaseline();
        }
        if !power.watchdog.watching(now) {
            return None;
        }
        let callbacks = signal.callbacks();
        let route_id = signal.route_id();
        if start_stalled(signal.since_start()?, callbacks) {
            if !signal.mark_failed() {
                return None;
            }
            diag::log(&format!("route {route_id}: no IO callback since the start"));
            return Some(PlatformEvent::RenderFailed { route_id });
        }
        if !stalled_mid_run(callbacks, signal.since_last_callback()) || !signal.mark_failed() {
            return None;
        }
        let report = power.watchdog.mid_run_stall(now);
        diag::log(&format!(
            "route {route_id}: IO stopped after {callbacks} callbacks ({report:?})"
        ));
        Some(match report {
            StallReport::Rebuild => PlatformEvent::RouteStalled { route_id },
            StallReport::Failed => PlatformEvent::RenderFailed { route_id },
        })
    }

    /// `CaptureWorks`, once per route, when its input first carries sound (Swift sets the
    /// permission granted then and stops checking); before that, `CaptureLooksDenied`, once per
    /// route, when the input stays silent while other apps play.
    fn check_capture(&self) -> Option<PlatformEvent> {
        let signal = self.running.borrow().upgrade()?;
        let since_signal = signal.since_signal();
        if since_signal.is_some() {
            return signal
                .mark_works_reported()
                .then_some(PlatformEvent::CaptureWorks);
        }
        if signal.denied_reported() {
            return None;
        }
        let since_first = signal.since_first_buffer()?;
        // The timing first: `others_playing` walks the HAL's process list.
        if !looks_denied(since_signal, true, since_first) || !hal::others_playing() {
            return None;
        }
        signal.set_denied_reported();
        Some(PlatformEvent::CaptureLooksDenied)
    }
}

fn lock(power: &Mutex<Power>) -> std::sync::MutexGuard<'_, Power> {
    power.lock().unwrap_or_else(|e| e.into_inner())
}

/// The next stall check re-baselines the running route, then waits out the grace period.
fn wake_at(power: &Mutex<Power>, at: Instant) {
    let mut p = lock(power);
    p.watchdog.did_wake(at);
    p.rebaseline = true;
}

impl Audio for MacAudio {
    /// Sorted by name, as in Swift's `OutputDeviceMonitor.available`.
    fn outputs(&self) -> Vec<OutputDevice> {
        let mut outputs = hal::output_devices();
        outputs.sort_by(|a, b| natural_cmp(&a.name, &b.name));
        outputs
    }

    fn current_output(&self) -> Option<OutputDevice> {
        hal::output_device(hal::default_output()?)
    }

    fn route_builder(&self, engine: SharedEngine) -> Box<dyn RouteBuilder> {
        Box::new(MacRouteBuilder {
            engine,
            events: self.events.clone(),
            mute: true,
            running: self.running.clone(),
        })
    }

    fn poll_events(&mut self) -> Vec<PlatformEvent> {
        let mut events = self.events.drain();
        for e in &events {
            match e {
                PlatformEvent::OutputsChanged => diag::log(&format!(
                    "outputs changed, default {:?}",
                    hal::default_output().and_then(hal::device_uid)
                )),
                // From the start thread or the IO block (a start error, a panic, a too large
                // buffer); the watchdog's own failures are logged in `check_stall`.
                e => diag::log(&format!("{e:?}")),
            }
        }
        events.extend(self.check_stall());
        events.extend(self.check_capture());
        events
    }
}
