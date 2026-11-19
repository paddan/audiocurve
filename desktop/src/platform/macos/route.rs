//! The system-wide route (Swift `CoreAudioRouteBuilder`, `ProcessTap`, `AggregateDevice` and
//! `RenderLoop`): a private stereo tap on every process but this one, muting what it taps → a
//! private aggregate device with the output as main subdevice → an IOProc running the `Engine`.
//!
//! The tap mutes the whole Mac while it exists, so every way out of a `MacRoute` (`tear_down`,
//! `Drop`, a failed `build`) destroys it; a panic in the IO block is caught and outputs silence.

use std::cell::{Cell, RefCell, UnsafeCell};
use std::ffi::CStr;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::ptr::{self, NonNull};
use std::rc::Rc;
use std::slice;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use audiocurve_core::route::{BuiltRoute, RouteBuildError, RouteBuilder};
use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{AllocAnyThread, Message};
use objc2_core_audio::{
    AudioConvertHostTimeToNanos, AudioConvertNanosToHostTime, AudioDeviceCreateIOProcIDWithBlock,
    AudioDeviceDestroyIOProcID, AudioDeviceIOProcID, AudioDeviceStart, AudioDeviceStop,
    AudioGetCurrentHostTime, AudioHardwareCreateAggregateDevice, AudioHardwareCreateProcessTap,
    AudioHardwareDestroyAggregateDevice, AudioHardwareDestroyProcessTap, CATapDescription,
    CATapMuteBehavior, kAudioAggregateDeviceIsPrivateKey, kAudioAggregateDeviceIsStackedKey,
    kAudioAggregateDeviceMainSubDeviceKey, kAudioAggregateDeviceNameKey,
    kAudioAggregateDeviceSubDeviceListKey, kAudioAggregateDeviceTapAutoStartKey,
    kAudioAggregateDeviceTapListKey, kAudioAggregateDeviceUIDKey,
    kAudioDevicePropertyBufferFrameSize, kAudioDevicePropertyBufferFrameSizeRange,
    kAudioDevicePropertyDeviceIsAlive, kAudioDevicePropertyNominalSampleRate,
    kAudioHardwarePropertyServiceRestarted, kAudioObjectUnknown, kAudioSubDeviceUIDKey,
    kAudioSubTapDriftCompensationKey, kAudioSubTapUIDKey,
};
use objc2_core_audio_types::{AudioBuffer, AudioBufferList, AudioTimeStamp, AudioValueRange};
use objc2_core_foundation::CFDictionary;
use objc2_foundation::{NSArray, NSDictionary, NSNumber, NSString};
use uuid::Uuid;

use super::audio::EventQueue;
use super::diag;
use super::hal::{self, HalError, Listener, ObjectId};
use crate::engine::{Engine, SharedEngine};
use crate::platform::PlatformEvent;

/// The IO buffer is at least this many frames.
pub const MIN_BUFFER_FRAMES: u32 = 256;

const UNKNOWN: ObjectId = kAudioObjectUnknown as ObjectId;

/// What the IO thread records about a running route, read by `MacAudio`'s capture and stall
/// checks.
pub struct RouteSignal {
    route_id: Uuid,
    /// When `AudioDeviceStart` returned `noErr` (set once, by the start thread).
    started: OnceLock<Instant>,
    callbacks: AtomicU64,
    /// Host time of the first IO callback (0: none yet).
    first_buffer: AtomicU64,
    /// Host time of the latest IO callback (0: none yet). Host time stands still in deep sleep
    /// but runs in dark wake; `rebaseline` moves it to the wake.
    last_callback: AtomicU64,
    /// Host time of the last input buffer with a non-zero sample (0: none yet).
    last_signal: AtomicU64,
    /// IO failed (start error or a panic); the route outputs silence from then on.
    failed: AtomicBool,
    denied_reported: AtomicBool,
    works_reported: AtomicBool,
}

impl RouteSignal {
    fn new(route_id: Uuid) -> RouteSignal {
        RouteSignal {
            route_id,
            started: OnceLock::new(),
            callbacks: AtomicU64::new(0),
            first_buffer: AtomicU64::new(0),
            last_callback: AtomicU64::new(0),
            last_signal: AtomicU64::new(0),
            failed: AtomicBool::new(false),
            denied_reported: AtomicBool::new(false),
            works_reported: AtomicBool::new(false),
        }
    }

    /// Test hook: a route started at `at` that has had no IO callback yet.
    #[doc(hidden)]
    pub fn started_at(route_id: Uuid, at: Instant) -> RouteSignal {
        let s = RouteSignal::new(route_id);
        let _ = s.started.set(at);
        s
    }

    /// Test hook: a route started a minute ago that has had IO callbacks since, the latest
    /// `since_last_callback` ago, and input sound `since_signal` ago (`None`: never).
    #[doc(hidden)]
    pub fn running(
        route_id: Uuid,
        since_last_callback: Duration,
        since_signal: Option<Duration>,
    ) -> RouteSignal {
        let minute = Duration::from_secs(60);
        let s = RouteSignal::started_at(route_id, Instant::now() - minute);
        s.callbacks.store(1000, Ordering::Relaxed);
        s.first_buffer.store(host_ago(minute), Ordering::Relaxed);
        s.last_callback
            .store(host_ago(since_last_callback), Ordering::Relaxed);
        if let Some(ago) = since_signal {
            s.last_signal.store(host_ago(ago), Ordering::Relaxed);
        }
        s
    }

    pub fn route_id(&self) -> Uuid {
        self.route_id
    }

    pub fn callbacks(&self) -> u64 {
        self.callbacks.load(Ordering::Relaxed)
    }

    /// Time since IO started successfully, or `None` while the start is pending or failed.
    pub fn since_start(&self) -> Option<Duration> {
        self.started.get().map(Instant::elapsed)
    }

    /// Marks the route failed; true only the first time (by the IO thread, the start thread or
    /// the stall check), so a route reports `RenderFailed` at most once.
    pub fn mark_failed(&self) -> bool {
        !self.failed.swap(true, Ordering::Relaxed)
    }

    /// Time since the first IO callback, or `None` before it.
    pub fn since_first_buffer(&self) -> Option<Duration> {
        since(self.first_buffer.load(Ordering::Relaxed))
    }

    /// On wake: counts the time since the last callback from now, so the sleep is not a stall.
    /// Does nothing before the first callback (that is the start check's case).
    pub fn rebaseline(&self) {
        let now = host_now();
        let _ = self
            .last_callback
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |t| {
                (t != 0).then_some(t.max(now))
            });
    }

    /// Time since the latest IO callback, or `None` before the first.
    pub fn since_last_callback(&self) -> Option<Duration> {
        since(self.last_callback.load(Ordering::Relaxed))
    }

    /// Time since the input last carried sound, or `None` if it never has.
    pub fn since_signal(&self) -> Option<Duration> {
        since(self.last_signal.load(Ordering::Relaxed))
    }

    pub fn denied_reported(&self) -> bool {
        self.denied_reported.load(Ordering::Relaxed)
    }

    pub fn set_denied_reported(&self) {
        self.denied_reported.store(true, Ordering::Relaxed);
    }

    /// Marks that this route's capture was seen working; true only the first time.
    pub fn mark_works_reported(&self) -> bool {
        !self.works_reported.swap(true, Ordering::Relaxed)
    }

    /// Pushes `RenderFailed` the first time only.
    fn fail(&self, events: &EventQueue) {
        if self.mark_failed() {
            events.push(PlatformEvent::RenderFailed {
                route_id: self.route_id,
            });
        }
    }
}

fn host_now() -> u64 {
    // SAFETY: reads the host clock.
    unsafe { AudioGetCurrentHostTime() }
}

/// The host time `ago` before now (at least 1, so it never reads as "none yet").
fn host_ago(ago: Duration) -> u64 {
    // SAFETY: a pure conversion.
    let ticks = unsafe { AudioConvertNanosToHostTime(ago.as_nanos() as u64) };
    host_now().saturating_sub(ticks).max(1)
}

fn since(host_time: u64) -> Option<Duration> {
    if host_time == 0 {
        return None;
    }
    let now = host_now();
    // SAFETY: a pure conversion.
    let nanos = unsafe { AudioConvertHostTimeToNanos(now.saturating_sub(host_time)) };
    Some(Duration::from_nanos(nanos))
}

type Teardown = Box<dyn FnOnce() + Send>;

/// The live route's teardown, for the panic hook: it holds only the HAL object IDs, so it can
/// run on any thread. A route fills it as it creates each object and clears it when it is torn
/// down; the slot holds the newest route, keyed by its ID.
static ROUTE_GUARD: Mutex<Option<(Uuid, Teardown)>> = Mutex::new(None);

thread_local! {
    /// This thread runs IO blocks, whose panics are caught and reported as `RenderFailed`.
    static RENDER_THREAD: Cell<bool> = const { Cell::new(false) };
}

/// The calling thread runs a route's IO block.
pub fn is_render_thread() -> bool {
    RENDER_THREAD.with(Cell::get)
}

/// Called first in every IO block (the route's and the test tone's).
pub(super) fn mark_render_thread() {
    RENDER_THREAD.with(|r| r.set(true));
}

/// Tears down the live route from any thread, without the `MacRoute`: for a panic, before the
/// process aborts. Returns whether there was one.
pub fn tear_down_registered_route() -> bool {
    let guard = ROUTE_GUARD.lock().unwrap_or_else(|e| e.into_inner()).take();
    guard.map(|(_, tear_down)| tear_down()).is_some()
}

fn set_guard(route_id: Uuid, tear_down: Teardown) {
    *ROUTE_GUARD.lock().unwrap_or_else(|e| e.into_inner()) = Some((route_id, tear_down));
}

fn clear_guard(route_id: Uuid) {
    let mut slot = ROUTE_GUARD.lock().unwrap_or_else(|e| e.into_inner());
    if slot.as_ref().is_some_and(|(id, _)| *id == route_id) {
        *slot = None;
    }
}

#[cfg(test)]
fn guard_holds(route_id: Uuid) -> bool {
    let slot = ROUTE_GUARD.lock().unwrap_or_else(|e| e.into_inner());
    slot.as_ref().is_some_and(|(id, _)| *id == route_id)
}

/// The process the tap must exclude: ours. Without it the tap would capture AudioCurve's own
/// output (feedback through the gain stage), so the build fails and the controller retries.
fn own_process_exclusion(me: Option<ObjectId>) -> Result<ObjectId, RouteBuildError> {
    me.ok_or_else(|| RouteBuildError("this process has no audio process object".into()))
}

/// Stops and destroys the IOProc, if any.
///
/// # Safety
/// `proc_id` is an IOProc on `aggregate`, or None.
unsafe fn destroy_io_proc(aggregate: ObjectId, proc_id: AudioDeviceIOProcID) {
    if proc_id.is_some() {
        // SAFETY: forwarded caller contract.
        unsafe {
            AudioDeviceStop(aggregate, proc_id);
            AudioDeviceDestroyIOProcID(aggregate, proc_id);
        }
    }
}

/// Destroys the aggregate and then the tap (which unmutes the tapped processes); `UNKNOWN`
/// skips one.
///
/// # Safety
/// The objects were created by this process and are not used afterwards.
unsafe fn destroy_devices(aggregate: ObjectId, tap: ObjectId) {
    if aggregate != UNKNOWN {
        // SAFETY: forwarded caller contract.
        unsafe { AudioHardwareDestroyAggregateDevice(aggregate) };
    }
    if tap != UNKNOWN {
        // SAFETY: forwarded caller contract.
        unsafe { AudioHardwareDestroyProcessTap(tap) };
    }
}

/// The newest route's signal, shared between the builder and `MacAudio` (UI thread only).
pub type RunningRoute = Rc<RefCell<Weak<RouteSignal>>>;

pub struct MacRouteBuilder {
    pub engine: SharedEngine,
    pub events: EventQueue,
    /// Mute what the tap captures: true in the app, false in hardware tests.
    pub mute: bool,
    pub running: RunningRoute,
}

impl MacRouteBuilder {
    pub fn new(engine: SharedEngine, events: EventQueue, mute: bool) -> Self {
        MacRouteBuilder {
            engine,
            events,
            mute,
            running: RunningRoute::default(),
        }
    }

    /// `build`, returning the concrete route.
    pub fn build_route(&mut self, device_uid: &str) -> Result<MacRoute, RouteBuildError> {
        let route = self.try_build_route(device_uid);
        match &route {
            Ok(r) => diag::log(&format!("route {} built on {device_uid}", r.id)),
            Err(e) => diag::log(&format!("route build on {device_uid} failed: {}", e.0)),
        }
        route
    }

    fn try_build_route(&mut self, device_uid: &str) -> Result<MacRoute, RouteBuildError> {
        let output = hal::device_by_uid(device_uid)
            .ok_or_else(|| RouteBuildError(format!("output {device_uid} not found")))?;
        // Every early return below drops `route`, which destroys whatever exists so far.
        let mut route = MacRoute::new(self.events.clone());
        let tap_uid = route.create_tap(self.mute)?;
        route.register_guard();
        route.create_aggregate(device_uid, &tap_uid)?;
        route.register_guard();
        set_buffer_frames(route.aggregate, MIN_BUFFER_FRAMES);

        let rate = hal::nominal_sample_rate(route.aggregate)
            .filter(|r| (8_000.0..=384_000.0).contains(r))
            .ok_or_else(|| RouteBuildError("the route's sample rate is unusable".into()))?;
        if self.engine.borrow().sample_rate() != rate {
            let engine = self.engine.borrow().with_sample_rate(rate);
            *self.engine.borrow_mut() = Arc::new(engine);
        }
        let engine = self.engine.borrow().clone();
        route.create_io_proc(engine)?;
        route.register_guard();

        let on_event = route.on_device_event.clone();
        let watched = [
            (output, kAudioDevicePropertyNominalSampleRate),
            (output, kAudioDevicePropertyDeviceIsAlive),
            (route.aggregate, kAudioDevicePropertyDeviceIsAlive),
            (hal::SYSTEM, kAudioHardwarePropertyServiceRestarted),
        ];
        route.listeners = watched
            .into_iter()
            .map(|(object, selector)| {
                let f = on_event.clone();
                hal::listen(object, selector, move || f())
            })
            .collect();

        route.start();
        *self.running.borrow_mut() = Arc::downgrade(&route.signal);
        Ok(route)
    }
}

impl RouteBuilder for MacRouteBuilder {
    fn build(&mut self, device_uid: &str) -> Result<Box<dyn BuiltRoute>, RouteBuildError> {
        Ok(Box::new(self.build_route(device_uid)?))
    }
}

fn build_error(e: HalError) -> RouteBuildError {
    RouteBuildError(e.to_string())
}

pub(super) type IoBlock = RcBlock<
    dyn Fn(
        NonNull<AudioTimeStamp>,
        NonNull<AudioBufferList>,
        NonNull<AudioTimeStamp>,
        NonNull<AudioBufferList>,
        NonNull<AudioTimeStamp>,
    ),
>;

/// A built route. Tearing it down (or dropping it) stops IO, destroys the IOProc, removes the
/// listeners, destroys the aggregate and then the tap, which unmutes the Mac.
pub struct MacRoute {
    id: Uuid,
    events: EventQueue,
    tap: ObjectId,
    aggregate: ObjectId,
    proc_id: AudioDeviceIOProcID,
    block: Option<IoBlock>,
    starting: Option<JoinHandle<()>>,
    listeners: Vec<Listener>,
    signal: Arc<RouteSignal>,
    /// Cleared on teardown, so a listener callback already queued on the main queue is a no-op.
    listening: Arc<AtomicBool>,
    on_device_event: Arc<dyn Fn() + Send + Sync>,
}

impl MacRoute {
    fn new(events: EventQueue) -> MacRoute {
        let listening = Arc::new(AtomicBool::new(true));
        let on_device_event: Arc<dyn Fn() + Send + Sync> = {
            let (events, listening) = (events.clone(), listening.clone());
            Arc::new(move || {
                if listening.load(Ordering::Relaxed) {
                    events.push(PlatformEvent::SampleRateChanged);
                }
            })
        };
        let id = Uuid::new_v4();
        MacRoute {
            id,
            events,
            tap: UNKNOWN,
            aggregate: UNKNOWN,
            proc_id: None,
            block: None,
            starting: None,
            listeners: Vec::new(),
            signal: Arc::new(RouteSignal::new(id)),
            listening,
            on_device_event,
        }
    }

    /// IO callbacks so far.
    pub fn callbacks(&self) -> u64 {
        self.signal.callbacks()
    }

    /// The aggregate device.
    pub fn aggregate(&self) -> ObjectId {
        self.aggregate
    }

    /// Test hook: what the device and system listeners do when they fire.
    pub fn simulate_device_event(&self) {
        (self.on_device_event)();
    }

    /// Makes the objects created so far the panic hook's to destroy.
    fn register_guard(&self) {
        let (aggregate, proc_id, tap) = (self.aggregate, self.proc_id, self.tap);
        set_guard(
            self.id,
            Box::new(move || {
                // SAFETY: this route's objects. `shut_down` clears the slot only after it
                // destroyed them, so a panic or second signal meanwhile may destroy one twice,
                // which the HAL rejects harmlessly.
                unsafe {
                    destroy_io_proc(aggregate, proc_id);
                    destroy_devices(aggregate, tap);
                }
            }),
        );
    }

    /// A private stereo tap on every process except this one. Returns its UID.
    fn create_tap(&mut self, mute: bool) -> Result<String, RouteBuildError> {
        let me = own_process_exclusion(hal::process_object(std::process::id() as i32))?;
        let excluded = NSArray::from_retained_slice(&[NSNumber::new_u32(me)]);
        let behavior = if mute {
            CATapMuteBehavior::MutedWhenTapped
        } else {
            CATapMuteBehavior::Unmuted
        };
        // SAFETY: a freshly allocated description, configured before use.
        let desc = unsafe {
            let desc = CATapDescription::initStereoGlobalTapButExcludeProcesses(
                CATapDescription::alloc(),
                &excluded,
            );
            desc.setName(&NSString::from_str("AudioCurve"));
            desc.setPrivate(true);
            desc.setMuteBehavior(behavior);
            desc
        };
        let mut tap = UNKNOWN;
        // SAFETY: a valid description and out-parameter.
        let status = unsafe { AudioHardwareCreateProcessTap(Some(&desc), &mut tap) };
        hal::check(status, "AudioHardwareCreateProcessTap").map_err(build_error)?;
        self.tap = tap;
        // SAFETY: plain getters.
        Ok(unsafe { desc.UUID().UUIDString() }.to_string())
    }

    /// A private aggregate: the tap as input, the output as main subdevice.
    fn create_aggregate(&mut self, output_uid: &str, tap_uid: &str) -> Result<(), RouteBuildError> {
        let route_uid = format!(
            "{}{}",
            hal::ROUTE_UID_PREFIX,
            Uuid::new_v4().to_string().to_uppercase()
        );
        let sub_device = dict(&[(kAudioSubDeviceUIDKey, string(output_uid))]);
        let sub_tap = dict(&[
            (
                kAudioSubTapDriftCompensationKey,
                any(NSNumber::new_bool(true)),
            ),
            (kAudioSubTapUIDKey, string(tap_uid)),
        ]);
        let description = dict(&[
            (kAudioAggregateDeviceNameKey, string("AudioCurve")),
            (kAudioAggregateDeviceUIDKey, string(&route_uid)),
            (kAudioAggregateDeviceMainSubDeviceKey, string(output_uid)),
            (
                kAudioAggregateDeviceIsPrivateKey,
                any(NSNumber::new_bool(true)),
            ),
            (
                kAudioAggregateDeviceIsStackedKey,
                any(NSNumber::new_bool(false)),
            ),
            (
                kAudioAggregateDeviceTapAutoStartKey,
                any(NSNumber::new_bool(true)),
            ),
            (
                kAudioAggregateDeviceSubDeviceListKey,
                any(NSArray::from_retained_slice(&[sub_device])),
            ),
            (
                kAudioAggregateDeviceTapListKey,
                any(NSArray::from_retained_slice(&[sub_tap])),
            ),
        ]);
        // SAFETY: `NSDictionary` is toll-free bridged to `CFDictionary`.
        let cf: &CFDictionary = unsafe { &*Retained::as_ptr(&description).cast::<CFDictionary>() };
        let mut device = UNKNOWN;
        // SAFETY: a valid description and out-parameter.
        let status = unsafe { AudioHardwareCreateAggregateDevice(cf, NonNull::from(&mut device)) };
        hal::check(status, "AudioHardwareCreateAggregateDevice").map_err(build_error)?;
        self.aggregate = device;
        Ok(())
    }

    fn create_io_proc(&mut self, engine: Arc<Engine>) -> Result<(), RouteBuildError> {
        let render = Render::new(engine, self.signal.clone(), self.events.clone());
        let block: IoBlock = RcBlock::new(
            move |_now: NonNull<AudioTimeStamp>,
                  input: NonNull<AudioBufferList>,
                  _input_time: NonNull<AudioTimeStamp>,
                  output: NonNull<AudioBufferList>,
                  _output_time: NonNull<AudioTimeStamp>| {
                // SAFETY: Core Audio's buffer lists, on the IOProc's one thread.
                unsafe { render.io(input, output) }
            },
        );
        let mut proc_id: AudioDeviceIOProcID = None;
        // SAFETY: the block stays alive in `self.block` until the IOProc is destroyed.
        let status = unsafe {
            AudioDeviceCreateIOProcIDWithBlock(
                NonNull::from(&mut proc_id),
                self.aggregate,
                None,
                ptr::from_ref(&*block).cast_mut(),
            )
        };
        self.block = Some(block);
        hal::check(status, "AudioDeviceCreateIOProcIDWithBlock").map_err(build_error)?;
        self.proc_id = proc_id;
        Ok(())
    }

    /// Starts IO off the main thread: the first start blocks while the system asks for the
    /// capture permission. A start error is reported like a render failure.
    fn start(&mut self) {
        let (device, proc_id) = (self.aggregate, self.proc_id);
        // SAFETY: the IOProc exists until `shut_down`, which joins this thread first.
        let start = spawn_start(self.signal.clone(), self.events.clone(), move || unsafe {
            AudioDeviceStart(device, proc_id)
        });
        match start {
            Ok(handle) => self.starting = Some(handle),
            Err(_) => self.signal.fail(&self.events),
        }
    }

    /// Idempotent; the order matters (the tap last, as it is what mutes the Mac).
    /// The guard stays registered until the devices are gone, so a panic or second signal while
    /// this waits for a blocked `AudioDeviceStart` (the first-run prompt) still unmutes the Mac.
    fn shut_down(&mut self) {
        if self.aggregate != UNKNOWN || self.tap != UNKNOWN {
            diag::log(&format!(
                "route {} torn down after {} callbacks",
                self.id,
                self.callbacks()
            ));
        }
        self.listening.store(false, Ordering::Relaxed);
        if let Some(start) = self.starting.take() {
            let _ = start.join();
        }
        // SAFETY: the IOProc created on this aggregate, or None.
        unsafe { destroy_io_proc(self.aggregate, self.proc_id) };
        self.proc_id = None;
        self.block = None;
        self.listeners.clear();
        // SAFETY: the aggregate and tap this route created, reset below so they go once.
        unsafe { destroy_devices(self.aggregate, self.tap) };
        self.aggregate = UNKNOWN;
        self.tap = UNKNOWN;
        clear_guard(self.id);
    }
}

impl BuiltRoute for MacRoute {
    fn id(&self) -> Uuid {
        self.id
    }

    fn tear_down(mut self: Box<Self>) {
        self.shut_down();
    }
}

impl Drop for MacRoute {
    fn drop(&mut self) {
        self.shut_down();
    }
}

/// Runs `start` (`AudioDeviceStart`) on its own thread. The start time is recorded only once it
/// returned `noErr`, so the stall watchdog never counts the time a first-run capture prompt
/// holds the start; an error is reported like a render failure.
fn spawn_start(
    signal: Arc<RouteSignal>,
    events: EventQueue,
    start: impl FnOnce() -> i32 + Send + 'static,
) -> std::io::Result<JoinHandle<()>> {
    std::thread::Builder::new()
        .name("audiocurve-route-start".into())
        .spawn(move || {
            let status = start();
            if status == 0 {
                let _ = signal.started.set(Instant::now());
                diag::log(&format!("route {} started", signal.route_id));
            } else {
                diag::log(&format!("route {} start failed: {status}", signal.route_id));
                signal.fail(&events);
            }
        })
}

/// Sets the IO buffer to the smallest size the device accepts that is at least `minimum`.
fn set_buffer_frames(device: ObjectId, minimum: u32) {
    let empty = AudioValueRange {
        mMinimum: 0.0,
        mMaximum: 0.0,
    };
    // SAFETY: the property is an `AudioValueRange`.
    if let Ok(range) = unsafe { hal::get(device, kAudioDevicePropertyBufferFrameSizeRange, empty) }
    {
        let frames = f64::from(minimum).max(range.mMinimum).ceil() as u32;
        // SAFETY: the property is a `UInt32`.
        let _ = unsafe {
            hal::set(
                device,
                kAudioDevicePropertyBufferFrameSize,
                frames.min(range.mMaximum as u32),
            )
        };
    }
}

fn string(s: &str) -> Retained<AnyObject> {
    any(NSString::from_str(s))
}

fn any<T: Message>(o: Retained<T>) -> Retained<AnyObject> {
    // SAFETY: every Objective-C object is an `AnyObject`.
    unsafe { Retained::cast_unchecked(o) }
}

fn dict(entries: &[(&CStr, Retained<AnyObject>)]) -> Retained<NSDictionary<NSString, AnyObject>> {
    let keys: Vec<Retained<NSString>> = entries
        .iter()
        .map(|(k, _)| NSString::from_str(k.to_str().expect("an ASCII key")))
        .collect();
    let keys: Vec<&NSString> = keys.iter().map(|k| &**k).collect();
    let objects: Vec<Retained<AnyObject>> = entries.iter().map(|(_, v)| v.clone()).collect();
    NSDictionary::from_retained_objects(&keys, &objects)
}

/// The IO block's state. It does not allocate or lock: the buffers are allocated up front.
struct Render {
    engine: Arc<Engine>,
    max_frames: usize,
    l: UnsafeCell<Box<[f32]>>,
    r: UnsafeCell<Box<[f32]>>,
    signal: Arc<RouteSignal>,
    events: EventQueue,
    /// Unit-test hook: `Engine::process` panics.
    #[cfg(test)]
    panic_in_process: AtomicBool,
}

impl Render {
    fn new(engine: Arc<Engine>, signal: Arc<RouteSignal>, events: EventQueue) -> Render {
        let max_frames = engine.max_frames();
        Render {
            engine,
            max_frames,
            l: UnsafeCell::new(vec![0.0; max_frames].into_boxed_slice()),
            r: UnsafeCell::new(vec![0.0; max_frames].into_boxed_slice()),
            signal,
            events,
            #[cfg(test)]
            panic_in_process: AtomicBool::new(false),
        }
    }

    fn fail(&self) {
        // The one exception to "no locks": a failure is reported once and IO goes silent.
        self.signal.fail(&self.events);
    }

    /// The IO block's body. Nothing may unwind into Core Audio.
    ///
    /// # Safety
    /// As `cycle`.
    unsafe fn io(&self, input: NonNull<AudioBufferList>, output: NonNull<AudioBufferList>) {
        mark_render_thread();
        // SAFETY: forwarded caller contract.
        let cycle = catch_unwind(AssertUnwindSafe(|| unsafe { self.cycle(input, output) }));
        if cycle.is_err() {
            // SAFETY: as above.
            zero(unsafe { buffers_mut(output) });
            self.fail();
        }
    }

    fn process(&self, l: &mut [f32], r: &mut [f32]) {
        #[cfg(test)]
        if self.panic_in_process.load(Ordering::Relaxed) {
            // Unwinds without the panic hook, so test output stays clean.
            std::panic::resume_unwind(Box::new("injected engine panic"));
        }
        // SAFETY: only the IOProc's one thread calls this, with at most `max_frames` frames.
        unsafe { self.engine.process(l, r) }
    }

    /// Tap input → `Engine::process` in place → output.
    ///
    /// # Safety
    /// Core Audio's buffer lists, called from one thread at a time.
    unsafe fn cycle(&self, input: NonNull<AudioBufferList>, output: NonNull<AudioBufferList>) {
        let now = host_now();
        let signal = &self.signal;
        signal.callbacks.fetch_add(1, Ordering::Relaxed);
        signal.last_callback.store(now, Ordering::Relaxed);
        let _ = signal
            .first_buffer
            .compare_exchange(0, now, Ordering::Relaxed, Ordering::Relaxed);
        // SAFETY: forwarded caller contract.
        let out = unsafe { buffers_mut(output) };
        let frames = frame_count(out);
        if frames == 0 || signal.failed.load(Ordering::Relaxed) {
            zero(out);
            return;
        }
        if frames > self.max_frames {
            // The engine cannot run this cycle: report it rather than stay silent and muted.
            zero(out);
            self.fail();
            return;
        }
        // SAFETY: only this (single) IO thread touches the buffers.
        let (l, r) = unsafe { (&mut *self.l.get(), &mut *self.r.get()) };
        let (l, r) = (&mut l[..frames], &mut r[..frames]);
        // SAFETY: forwarded caller contract.
        if unsafe { read_input(buffers(input), l, r) } {
            signal.last_signal.store(now, Ordering::Relaxed);
        }
        let processed = catch_unwind(AssertUnwindSafe(|| self.process(l, r)));
        if processed.is_err() {
            zero(out);
            self.fail();
            return;
        }
        // SAFETY: forwarded caller contract.
        unsafe { write_output(out, l, r) };
    }
}

/// # Safety
/// `list` must be a valid buffer list.
unsafe fn buffers<'a>(list: NonNull<AudioBufferList>) -> &'a [AudioBuffer] {
    let list = list.as_ptr();
    // SAFETY: `mNumberBuffers` buffers follow in place.
    unsafe {
        slice::from_raw_parts(
            ptr::addr_of!((*list).mBuffers).cast::<AudioBuffer>(),
            (*list).mNumberBuffers as usize,
        )
    }
}

/// # Safety
/// `list` must be a valid, writable buffer list.
pub(super) unsafe fn buffers_mut<'a>(list: NonNull<AudioBufferList>) -> &'a mut [AudioBuffer] {
    let list = list.as_ptr();
    // SAFETY: `mNumberBuffers` buffers follow in place.
    unsafe {
        slice::from_raw_parts_mut(
            ptr::addr_of_mut!((*list).mBuffers).cast::<AudioBuffer>(),
            (*list).mNumberBuffers as usize,
        )
    }
}

pub(super) const SAMPLE: usize = size_of::<f32>();

pub(super) fn frame_count(out: &[AudioBuffer]) -> usize {
    match out.first() {
        Some(b) if b.mNumberChannels > 0 => {
            b.mDataByteSize as usize / (SAMPLE * b.mNumberChannels as usize)
        }
        _ => 0,
    }
}

pub(super) fn zero(out: &mut [AudioBuffer]) {
    for b in out {
        if !b.mData.is_null() {
            // SAFETY: the buffer holds `mDataByteSize` bytes.
            unsafe { ptr::write_bytes(b.mData.cast::<u8>(), 0, b.mDataByteSize as usize) };
        }
    }
}

/// Copies the tap input into `l`/`r` (interleaved, two mono buffers, or one mono buffer).
/// Returns whether any sample was non-zero.
///
/// # Safety
/// Each buffer's `mData` must hold `mDataByteSize` bytes of `f32`.
unsafe fn read_input(input: &[AudioBuffer], l: &mut [f32], r: &mut [f32]) -> bool {
    let frames = l.len();
    let Some(first) = input.first().filter(|b| !b.mData.is_null()) else {
        l.fill(0.0);
        r.fill(0.0);
        return false;
    };
    let ch = first.mNumberChannels.max(1) as usize;
    // A buffer shorter than the output cycle is never read past its end.
    if (first.mDataByteSize as usize) / (SAMPLE * ch) < frames {
        l.fill(0.0);
        r.fill(0.0);
        return false;
    }
    // SAFETY: checked above to hold at least `frames * ch` samples.
    let d0 = unsafe { slice::from_raw_parts(first.mData.cast::<f32>(), frames * ch) };
    if ch >= 2 {
        for (i, (l, r)) in l.iter_mut().zip(r.iter_mut()).enumerate() {
            *l = d0[i * ch];
            *r = d0[i * ch + 1];
        }
    } else {
        let d1 = input
            .get(1)
            .filter(|b| !b.mData.is_null() && b.mDataByteSize as usize >= frames * SAMPLE)
            // SAFETY: checked to hold at least `frames` samples.
            .map(|b| unsafe { slice::from_raw_parts(b.mData.cast::<f32>(), frames) })
            .unwrap_or(d0);
        l.copy_from_slice(d0);
        r.copy_from_slice(d1);
    }
    l.iter().chain(r.iter()).any(|&x| x != 0.0)
}

/// Writes `l`/`r` to the device buffers, whatever their layout; extra channels get silence.
///
/// # Safety
/// Each buffer's `mData` must hold `mDataByteSize` writable bytes of `f32`.
unsafe fn write_output(out: &mut [AudioBuffer], l: &[f32], r: &[f32]) {
    let count = out.len();
    for (b, buf) in out.iter_mut().enumerate() {
        if buf.mData.is_null() {
            continue;
        }
        let ch = buf.mNumberChannels.max(1) as usize;
        // SAFETY: the buffer holds `mDataByteSize` bytes.
        let d = unsafe {
            slice::from_raw_parts_mut(buf.mData.cast::<f32>(), buf.mDataByteSize as usize / SAMPLE)
        };
        let written = if ch >= 2 {
            let frames = l.len().min(d.len() / ch);
            for (i, frame) in d.chunks_exact_mut(ch).take(frames).enumerate() {
                frame[0] = l[i];
                frame[1] = r[i];
                frame[2..].fill(0.0);
            }
            frames * ch
        } else if count == 1 {
            let frames = l.len().min(d.len());
            for (i, s) in d[..frames].iter_mut().enumerate() {
                *s = 0.5 * (l[i] + r[i]);
            }
            frames
        } else {
            match b {
                0 | 1 => {
                    let src = if b == 0 { l } else { r };
                    let frames = src.len().min(d.len());
                    d[..frames].copy_from_slice(&src[..frames]);
                    frames
                }
                _ => 0,
            }
        };
        d[written..].fill(0.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An `AudioBufferList` with two buffers.
    #[repr(C)]
    struct List2 {
        count: u32,
        buffers: [AudioBuffer; 2],
    }

    fn mono(data: &mut [f32]) -> AudioBuffer {
        AudioBuffer {
            mNumberChannels: 1,
            mDataByteSize: (data.len() * SAMPLE) as u32,
            mData: data.as_mut_ptr().cast(),
        }
    }

    fn render() -> (Render, EventQueue) {
        let events = EventQueue::new(Arc::new(|| {}));
        let signal = Arc::new(RouteSignal::new(Uuid::new_v4()));
        let r = Render::new(Arc::new(Engine::new(48_000.0, 512)), signal, events.clone());
        (r, events)
    }

    /// Runs one 256-frame cycle: input 0.5 on both channels, output prefilled with 1.0.
    fn run(r: &Render) -> (Vec<f32>, Vec<f32>) {
        run_frames(r, 256)
    }

    fn run_frames(r: &Render, frames: usize) -> (Vec<f32>, Vec<f32>) {
        let (mut in_l, mut in_r) = (vec![0.5f32; frames], vec![0.5f32; frames]);
        let (mut out_l, mut out_r) = (vec![1.0f32; frames], vec![1.0f32; frames]);
        let mut input = List2 {
            count: 2,
            buffers: [mono(&mut in_l), mono(&mut in_r)],
        };
        let mut output = List2 {
            count: 2,
            buffers: [mono(&mut out_l), mono(&mut out_r)],
        };
        // SAFETY: two valid buffers of `frames` frames each, laid out as an `AudioBufferList`.
        unsafe {
            r.io(
                NonNull::from(&mut input).cast(),
                NonNull::from(&mut output).cast(),
            )
        };
        (out_l, out_r)
    }

    fn failures(events: &EventQueue) -> usize {
        events
            .drain()
            .iter()
            .filter(|e| matches!(e, PlatformEvent::RenderFailed { .. }))
            .count()
    }

    #[test]
    fn a_cycle_counts_and_records_the_input_signal() {
        let (r, events) = render();
        let (l, _) = run(&r);
        assert_eq!(r.signal.callbacks(), 1);
        assert!(r.signal.since_first_buffer().is_some());
        assert!(r.signal.since_signal().is_some());
        assert!(l.iter().all(|x| x.is_finite() && *x != 1.0));
        assert_eq!(failures(&events), 0);
    }

    /// The watchdog counts from a successful start only: while `AudioDeviceStart` blocks (the
    /// first-run capture prompt), there is no start time and no `RenderFailed`.
    #[test]
    fn a_pending_start_has_no_start_time_and_never_stalls() {
        use crate::platform::Audio;
        use crate::platform::macos::audio::MacAudio;
        use std::sync::mpsc;

        let mut audio = MacAudio::new(Arc::new(|| {}));
        let signal = Arc::new(RouteSignal::new(Uuid::new_v4()));
        audio.set_running_route(&signal);
        let events = EventQueue::new(Arc::new(|| {}));
        let (release, held) = mpsc::channel::<i32>();
        let start =
            spawn_start(signal.clone(), events.clone(), move || held.recv().unwrap()).unwrap();
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(signal.since_start(), None);
        assert!(audio.poll_events().is_empty());

        release.send(0).unwrap();
        start.join().unwrap();
        assert!(signal.since_start().is_some());
        assert!(audio.poll_events().is_empty(), "not 5 s yet");
        assert_eq!(failures(&events), 0);
    }

    #[test]
    fn a_failed_start_reports_once_and_has_no_start_time() {
        let events = EventQueue::new(Arc::new(|| {}));
        let signal = Arc::new(RouteSignal::new(Uuid::new_v4()));
        spawn_start(signal.clone(), events.clone(), || -1)
            .unwrap()
            .join()
            .unwrap();
        assert_eq!(signal.since_start(), None);
        assert_eq!(failures(&events), 1);
    }

    #[test]
    fn an_engine_panic_outputs_silence_and_reports_once() {
        let (r, events) = render();
        r.panic_in_process.store(true, Ordering::Relaxed);
        let (l, rr) = run(&r);
        assert!(l.iter().chain(&rr).all(|&x| x == 0.0));
        assert_eq!(failures(&events), 1);
        r.panic_in_process.store(false, Ordering::Relaxed);
        let (l, rr) = run(&r);
        assert!(l.iter().chain(&rr).all(|&x| x == 0.0), "stays silent");
        assert_eq!(failures(&events), 0, "reported once");
    }

    /// A cycle longer than the engine's buffers cannot be processed: it reports the failure
    /// (once) instead of going silent forever with the Mac muted.
    #[test]
    fn a_cycle_over_max_frames_outputs_silence_and_reports_once() {
        let (r, events) = render();
        let (l, rr) = run_frames(&r, 1024);
        assert!(l.iter().chain(&rr).all(|&x| x == 0.0));
        assert_eq!(failures(&events), 1);
        let (l, _) = run(&r);
        assert!(l.iter().all(|&x| x == 0.0), "stays silent");
        assert_eq!(failures(&events), 0, "reported once");
    }

    #[test]
    fn an_empty_cycle_is_silent_but_not_a_failure() {
        let (r, events) = render();
        run_frames(&r, 0);
        assert_eq!(failures(&events), 0);
    }

    /// Without our own process object the tap would capture AudioCurve's output too (feedback
    /// through the gain stage), so the build fails and the controller retries.
    #[test]
    fn the_tap_needs_our_process_object() {
        assert!(own_process_exclusion(None).is_err());
        assert_eq!(own_process_exclusion(Some(42)).unwrap(), 42);
    }

    /// While `shut_down` waits for a blocked `AudioDeviceStart` (the first-run prompt), the
    /// panic and second-signal teardown must still find the route.
    #[test]
    fn the_guard_holds_the_route_while_shut_down_joins_the_start() {
        let mut route = MacRoute::new(EventQueue::new(Arc::new(|| {})));
        // No HAL objects: the guard's teardown destroys nothing.
        route.register_guard();
        let id = route.id;
        let held = Arc::new(AtomicBool::new(false));
        let seen = held.clone();
        route.starting = Some(std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(50));
            seen.store(guard_holds(id), Ordering::Relaxed);
        }));
        route.shut_down();
        assert!(
            held.load(Ordering::Relaxed),
            "the guard was empty during the join"
        );
        assert!(!guard_holds(id), "cleared once the route is gone");
    }
}
