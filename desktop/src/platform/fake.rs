//! A scripted platform for tests and for running the app without audio hardware.
//!
//! `fake()` returns the `Platform` and a `FakeControl`, a clonable handle that steers it.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use audiocurve_core::outputs::OutputDevice;
use audiocurve_core::profile::Ear;
use audiocurve_core::route::{BuiltRoute, RouteBuildError, RouteBuilder};
use audiocurve_core::volume::{ElementRestore, VolumeSnapshot};
use uuid::Uuid;

use super::{
    Activity, Audio, Platform, PlatformEvent, System, TestTone, Tray, TrayIconState, Volume,
};
use crate::engine::{Engine, SharedEngine};
use crate::model::menu::{MenuAction, MenuItem};

const SAMPLE_RATE: f64 = 48_000.0;
const NOISE_DBFS: f32 = -20.0;
const REALTIME_BLOCK: usize = 480;

/// Pink noise (Paul Kellet's filter) from a xorshift generator, at about -20 dBFS RMS.
struct Noise {
    state: u64,
    b: [f32; 7],
    gain: f32,
}

impl Noise {
    fn new() -> Self {
        // Calibrated by `noise_is_about_minus_20_dbfs`.
        Noise {
            state: 0x9E37_79B9_7F4A_7C15,
            b: [0.0; 7],
            gain: 10f32.powf(NOISE_DBFS / 20.0) * 0.575,
        }
    }

    fn white(&mut self) -> f32 {
        self.state ^= self.state << 13;
        self.state ^= self.state >> 7;
        self.state ^= self.state << 17;
        (self.state >> 40) as f32 / (1u64 << 23) as f32 - 1.0
    }

    fn next(&mut self) -> f32 {
        let w = self.white();
        let b = &mut self.b;
        b[0] = 0.99886 * b[0] + w * 0.0555179;
        b[1] = 0.99332 * b[1] + w * 0.0750759;
        b[2] = 0.96900 * b[2] + w * 0.153_852;
        b[3] = 0.86650 * b[3] + w * 0.3104856;
        b[4] = 0.55000 * b[4] + w * 0.5329522;
        b[5] = -0.7616 * b[5] - w * 0.0168980;
        let pink = b[0] + b[1] + b[2] + b[3] + b[4] + b[5] + b[6] + w * 0.5362;
        b[6] = w * 0.115926;
        pink * self.gain
    }
}

/// The noise generator doubles as the lock that keeps `process` on one thread at a time.
type Source = Arc<Mutex<Noise>>;

fn run_noise(engine: &Engine, source: &Source, frames: usize) {
    let mut noise = source.lock().unwrap_or_else(|e| e.into_inner());
    let block = engine.max_frames().min(512);
    let mut done = 0;
    while done < frames {
        let n = block.min(frames - done);
        let mut l: Vec<f32> = (0..n).map(|_| noise.next()).collect();
        let mut r: Vec<f32> = (0..n).map(|_| noise.next()).collect();
        // SAFETY: the `Mutex` held above allows one caller at a time; `n <= max_frames`.
        unsafe { engine.process(&mut l, &mut r) };
        done += n;
    }
}

struct Running {
    id: Uuid,
    engine: Arc<Engine>,
    source: Source,
}

type SetMaxHook = Box<dyn FnMut(&str)>;

struct FakeState {
    outputs: Vec<OutputDevice>,
    current: Option<String>,
    fail_builds: usize,
    events: Vec<PlatformEvent>,
    muted: BTreeSet<String>,
    others_playing: bool,
    volume_log: Vec<String>,
    tone_log: Vec<String>,
    route_log: Vec<String>,
    running: Option<Running>,
    realtime: Arc<AtomicBool>,
    launch_at_login: bool,
    tone_playing: bool,
    fail_restores: usize,
    on_set_max: Option<SetMaxHook>,
    fail_set_max: bool,
    fail_tone_starts: usize,
    volume_settable: bool,
    sample_rate: f64,
    /// Tone and volume calls in the order they happened.
    event_log: Vec<String>,
    menu: Vec<MenuItem>,
    tray_visible: bool,
    /// Every icon the app gave the tray, in order.
    tray_icons: Vec<TrayIconState>,
    dock_icon: Option<bool>,
    clicks: Vec<MenuAction>,
}

impl FakeState {
    fn log_tone(&mut self, e: String) {
        self.event_log.push(format!("tone:{e}"));
        self.tone_log.push(e);
    }

    fn log_volume(&mut self, e: String) {
        self.event_log.push(format!("volume:{e}"));
        self.volume_log.push(e);
    }
}

/// Steers the fake platform from a test (or from `main`).
#[derive(Clone)]
pub struct FakeControl(Rc<RefCell<FakeState>>);

impl FakeControl {
    pub fn set_outputs(&self, outputs: Vec<OutputDevice>, current: Option<&str>) {
        let mut s = self.0.borrow_mut();
        s.outputs = outputs;
        s.current = current.map(str::to_owned);
    }

    pub fn fail_next_builds(&self, n: usize) {
        self.0.borrow_mut().fail_builds = n;
    }

    pub fn emit(&self, e: PlatformEvent) {
        self.0.borrow_mut().events.push(e);
    }

    pub fn set_muted(&self, uid: &str, muted: bool) {
        let mut s = self.0.borrow_mut();
        if muted {
            s.muted.insert(uid.to_owned());
        } else {
            s.muted.remove(uid);
        }
    }

    pub fn set_others_playing(&self, on: bool) {
        self.0.borrow_mut().others_playing = on;
    }

    /// Makes the next `n` volume restores fail on every element (still logged as `restore:<uid>`).
    pub fn fail_next_restores(&self, n: usize) {
        self.0.borrow_mut().fail_restores = n;
    }

    /// Calls `f` with the device UID on every `set_max`, before it is logged.
    pub fn on_set_max(&self, f: impl FnMut(&str) + 'static) {
        self.0.borrow_mut().on_set_max = Some(Box::new(f));
    }

    /// Makes `set_max` fail (still logged as `max:<uid>`) while on.
    pub fn fail_set_max(&self, on: bool) {
        self.0.borrow_mut().fail_set_max = on;
    }

    /// Makes the next `n` test tone `start`s fail (still logged as `start:<uid>`).
    pub fn fail_next_tone_starts(&self, n: usize) {
        self.0.borrow_mut().fail_tone_starts = n;
    }

    /// An output whose volume cannot be set: `is_settable` is false and there is no snapshot.
    pub fn set_volume_settable(&self, on: bool) {
        self.0.borrow_mut().volume_settable = on;
    }

    /// The sample rate the test tone reports for every output.
    pub fn set_sample_rate(&self, hz: f64) {
        self.0.borrow_mut().sample_rate = hz;
    }

    /// The test tone generator stops by itself (`is_playing` turns false), as when its device
    /// goes away.
    pub fn stop_generator(&self) {
        self.0.borrow_mut().tone_playing = false;
    }

    /// Tone and volume calls in order, as `tone:<entry>` and `volume:<entry>` with the entries of
    /// `tone_log` and `volume_log`.
    pub fn event_log(&self) -> Vec<String> {
        self.0.borrow().event_log.clone()
    }

    /// The menu the app last gave the tray.
    pub fn menu(&self) -> Vec<MenuItem> {
        self.0.borrow().menu.clone()
    }

    pub fn tray_visible(&self) -> bool {
        self.0.borrow().tray_visible
    }

    /// Every menu bar icon the app set, in order.
    pub fn tray_icons(&self) -> Vec<TrayIconState> {
        self.0.borrow().tray_icons.clone()
    }

    /// Whether the Dock icon was last shown; None before the app set it.
    pub fn dock_icon(&self) -> Option<bool> {
        self.0.borrow().dock_icon
    }

    /// Clicks a menu item; the app handles it on its next tick.
    pub fn click(&self, a: MenuAction) {
        self.0.borrow_mut().clicks.push(a);
    }

    pub fn volume_log(&self) -> Vec<String> {
        self.0.borrow().volume_log.clone()
    }

    pub fn tone_log(&self) -> Vec<String> {
        self.0.borrow().tone_log.clone()
    }

    pub fn route_log(&self) -> Vec<String> {
        self.0.borrow().route_log.clone()
    }

    /// Lets running routes also be fed noise in real time from a background thread. Off by
    /// default; tests never turn it on. Applies to routes built afterwards.
    pub fn set_realtime(&self, on: bool) {
        self.0.borrow().realtime.store(on, Ordering::Relaxed);
    }

    /// The ID of the route that is running now, for a `RenderFailed` event.
    pub fn running_route_id(&self) -> Option<Uuid> {
        self.0.borrow().running.as_ref().map(|r| r.id)
    }

    /// Runs `frames` frames of -20 dBFS pink noise through the running route's engine, now.
    pub fn pump(&self, frames: usize) {
        let target = self
            .0
            .borrow()
            .running
            .as_ref()
            .map(|r| (r.engine.clone(), r.source.clone()));
        if let Some((engine, source)) = target {
            run_noise(&engine, &source, frames);
        }
    }
}

fn device(uid: &str, name: &str) -> OutputDevice {
    OutputDevice {
        uid: uid.into(),
        name: name.into(),
    }
}

pub fn fake() -> (Platform, FakeControl) {
    let state = Rc::new(RefCell::new(FakeState {
        outputs: vec![
            device("airpods", "AirPods Pro"),
            device("aids", "Hearing Aids"),
            device("speakers", "Mac Studio Speakers"),
        ],
        current: Some("airpods".into()),
        fail_builds: 0,
        events: Vec::new(),
        muted: BTreeSet::new(),
        others_playing: false,
        volume_log: Vec::new(),
        tone_log: Vec::new(),
        route_log: Vec::new(),
        running: None,
        realtime: Arc::new(AtomicBool::new(false)),
        launch_at_login: false,
        tone_playing: false,
        fail_restores: 0,
        on_set_max: None,
        fail_set_max: false,
        fail_tone_starts: 0,
        volume_settable: true,
        sample_rate: SAMPLE_RATE,
        event_log: Vec::new(),
        menu: Vec::new(),
        tray_visible: true,
        tray_icons: Vec::new(),
        dock_icon: None,
        clicks: Vec::new(),
    }));
    let c = FakeControl(state);
    let platform = Platform {
        audio: Box::new(FakeAudio(c.clone())),
        volume: Box::new(FakeVolume(c.clone())),
        tone: Box::new(FakeTone(c.clone())),
        activity: Box::new(FakeActivity(c.clone())),
        system: Box::new(FakeSystem(c.clone())),
        tray: Box::new(FakeTray(c.clone())),
    };
    (platform, c)
}

struct FakeAudio(FakeControl);

impl Audio for FakeAudio {
    fn outputs(&self) -> Vec<OutputDevice> {
        self.0.0.borrow().outputs.clone()
    }

    fn current_output(&self) -> Option<OutputDevice> {
        let s = self.0.0.borrow();
        let uid = s.current.as_deref()?;
        s.outputs.iter().find(|o| o.uid == uid).cloned()
    }

    fn route_builder(&self, engine: SharedEngine) -> Box<dyn RouteBuilder> {
        Box::new(FakeBuilder {
            control: self.0.clone(),
            engine,
        })
    }

    fn poll_events(&mut self) -> Vec<PlatformEvent> {
        std::mem::take(&mut self.0.0.borrow_mut().events)
    }
}

struct FakeBuilder {
    control: FakeControl,
    engine: SharedEngine,
}

impl RouteBuilder for FakeBuilder {
    fn build(&mut self, device_uid: &str) -> Result<Box<dyn BuiltRoute>, RouteBuildError> {
        let mut s = self.control.0.borrow_mut();
        if s.fail_builds > 0 {
            s.fail_builds -= 1;
            return Err(RouteBuildError(format!(
                "fake build failure for {device_uid}"
            )));
        }
        let engine = self.engine.borrow().clone();
        let source: Source = Arc::new(Mutex::new(Noise::new()));
        let id = Uuid::new_v4();
        s.route_log.push(format!("build:{device_uid}"));
        s.running = Some(Running {
            id,
            engine: engine.clone(),
            source: source.clone(),
        });

        let stop = Arc::new(AtomicBool::new(false));
        let thread = s.realtime.load(Ordering::Relaxed).then(|| {
            let stop = stop.clone();
            std::thread::spawn(move || {
                while !stop.load(Ordering::Relaxed) {
                    run_noise(&engine, &source, REALTIME_BLOCK);
                    std::thread::sleep(Duration::from_millis(10));
                }
            })
        });
        Ok(Box::new(FakeRoute {
            id,
            uid: device_uid.to_owned(),
            control: self.control.clone(),
            stop,
            thread,
        }))
    }
}

struct FakeRoute {
    id: Uuid,
    uid: String,
    control: FakeControl,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl BuiltRoute for FakeRoute {
    fn id(&self) -> Uuid {
        self.id
    }

    fn tear_down(self: Box<Self>) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread {
            let _ = t.join();
        }
        let mut s = self.control.0.borrow_mut();
        s.route_log.push(format!("teardown:{}", self.uid));
        if s.running.as_ref().is_some_and(|r| r.id == self.id) {
            s.running = None;
        }
    }
}

struct FakeVolume(FakeControl);

impl Volume for FakeVolume {
    fn snapshot(&self, uid: &str) -> Option<VolumeSnapshot> {
        if !self.0.0.borrow().volume_settable {
            return None;
        }
        Some(VolumeSnapshot {
            device_uid: uid.into(),
            scalars: BTreeMap::from([(0, 0.5)]),
            failed_attempts: 0,
        })
    }

    fn set_max(&mut self, uid: &str) -> bool {
        // The hook runs without the state borrowed, so it may read the fake.
        let hook = self.0.0.borrow_mut().on_set_max.take();
        if let Some(mut f) = hook {
            f(uid);
            self.0.0.borrow_mut().on_set_max = Some(f);
        }
        let mut s = self.0.0.borrow_mut();
        s.log_volume(format!("max:{uid}"));
        !s.fail_set_max
    }

    fn restore(&mut self, s: &VolumeSnapshot) -> BTreeMap<u32, ElementRestore> {
        let mut state = self.0.0.borrow_mut();
        state.log_volume(format!("restore:{}", s.device_uid));
        let outcome = if state.fail_restores > 0 {
            state.fail_restores -= 1;
            ElementRestore::Failed
        } else {
            ElementRestore::Restored
        };
        s.scalars.keys().map(|k| (*k, outcome)).collect()
    }

    fn is_muted(&self, uid: &str) -> bool {
        self.0.0.borrow().muted.contains(uid)
    }

    fn is_settable(&self, _uid: &str) -> bool {
        self.0.0.borrow().volume_settable
    }
}

struct FakeTone(FakeControl);

impl TestTone for FakeTone {
    fn start(&mut self, uid: &str) -> Result<(), String> {
        let mut s = self.0.0.borrow_mut();
        s.log_tone(format!("start:{uid}"));
        if s.fail_tone_starts > 0 {
            s.fail_tone_starts -= 1;
            return Err(format!("fake start failure for {uid}"));
        }
        s.tone_playing = true;
        Ok(())
    }

    fn play(&mut self, ear: Ear, frequency_hz: f64, level_dbfs: Option<f64>) {
        let level = level_dbfs.map_or("off".to_owned(), |l| format!("{l}"));
        self.0
            .0
            .borrow_mut()
            .log_tone(format!("play:{ear:?}:{frequency_hz}:{level}"));
    }

    fn is_playing(&self) -> bool {
        self.0.0.borrow().tone_playing
    }

    fn stop(&mut self) {
        let mut s = self.0.0.borrow_mut();
        s.log_tone("stop".into());
        s.tone_playing = false;
    }

    fn sample_rate(&self, _uid: &str) -> f64 {
        self.0.0.borrow().sample_rate
    }
}

struct FakeActivity(FakeControl);

impl Activity for FakeActivity {
    fn others_playing(&self) -> bool {
        self.0.0.borrow().others_playing
    }
}

struct FakeSystem(FakeControl);

impl System for FakeSystem {
    fn launch_at_login(&self) -> bool {
        self.0.0.borrow().launch_at_login
    }

    fn set_launch_at_login(&mut self, on: bool) -> Result<(), String> {
        self.0.0.borrow_mut().launch_at_login = on;
        Ok(())
    }

    fn set_dock_icon(&mut self, shown: bool) {
        self.0.0.borrow_mut().dock_icon = Some(shown);
    }

    fn open_capture_settings(&mut self) {}

    fn app_name(&self, bundle_id: &str) -> String {
        bundle_id.rsplit('.').next().unwrap_or(bundle_id).to_owned()
    }
}

struct FakeTray(FakeControl);

impl Tray for FakeTray {
    fn set_menu(&mut self, items: &[MenuItem]) {
        self.0.0.borrow_mut().menu = items.to_vec();
    }

    fn set_visible(&mut self, visible: bool) {
        self.0.0.borrow_mut().tray_visible = visible;
    }

    fn set_icon(&mut self, icon: TrayIconState) {
        self.0.0.borrow_mut().tray_icons.push(icon);
    }

    fn poll_actions(&mut self) -> Vec<MenuAction> {
        std::mem::take(&mut self.0.0.borrow_mut().clicks)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noise_is_about_minus_20_dbfs() {
        let mut n = Noise::new();
        let count = 480_000;
        let ms: f32 = (0..count).map(|_| n.next().powi(2)).sum::<f32>() / count as f32;
        let db = 10.0 * ms.log10();
        assert!((db - NOISE_DBFS).abs() < 3.0, "{db}");
    }
}
