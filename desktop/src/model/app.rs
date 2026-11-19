//! The app's state: on/off, the route, outputs and their assignments, the engine and the
//! instance heartbeat (Swift `AppModel`, plus the per-output part of `AudioCurveAudioUnit`).
//! Holds no platform code; the caller passes time in as `Instant`s.

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use audiocurve_core::heartbeat::InstanceHeartbeat;
use audiocurve_core::outputs::{
    ActiveOutputs, OutputAssignments, OutputDevice, OutputSwitchAction, output_switch_actions,
};
use audiocurve_core::route::{
    PermissionState, PluginLoadState, RouteController, RouteEvents, RouteFailure, RouteInputs,
    RouteTarget,
};
use audiocurve_core::store::ProfileStore;
use audiocurve_core::store::types::WorkingCopy;
use audiocurve_dsp::Params;
use uuid::Uuid;

use crate::engine::{Engine, SharedEngine};
use crate::i18n::{self, tr};
use crate::lifecycle;
use crate::model::auto_level::{AutoLevel, AutoLevelUi};
use crate::model::editor::Editor;
use crate::model::hearing::{Env, HearingSession, PauseReason, clock_random};
use crate::model::levels::Levels;
use crate::model::menu::{MenuAction, MenuItem, build_menu, tray_icon_for};
use crate::model::settings::{AppMode, Settings};
use crate::platform::{Platform, PlatformEvent, TrayIconState};

pub const BUNDLE_ID: &str = "se.lindefors.audiocurve";
const SAMPLE_RATE: f64 = 48_000.0;
const MAX_FRAMES: usize = 4096;
/// Level changes are saved (to `levels.json` and an assigned output) this long after the last change.
const LEVELS_SAVE_DELAY: Duration = Duration::from_millis(500);
/// A pending volume that could not be restored yet (device absent or not ready) is retried this often.
const VOLUME_RETRY_INTERVAL: Duration = Duration::from_secs(2);

/// Something the menu asks of the window system, for the UI to act on (see `take_ui_requests`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UiRequest {
    OpenWindow,
    OpenSettings,
}

pub struct Launch {
    pub from_login_item: bool,
}

struct PendingLevelsSave {
    uid: String,
    at: Instant,
}

pub struct AppModel {
    pub settings: Settings,
    pub levels: Levels,
    pub editor: Editor,
    pub bypass: bool,
    pub window_open: bool,
    pub current_output: Option<OutputDevice>,
    pub available_outputs: Vec<OutputDevice>,
    pub active_outputs: ActiveOutputs,
    pub assignments: OutputAssignments,
    /// Bundle IDs of other hosts that process the sound now.
    pub other_hosts: Vec<String>,
    /// A hearing test owns the output: the route is paused.
    pub testing: bool,
    platform: Platform,
    store: ProfileStore,
    engine: SharedEngine,
    controller: RouteController,
    permission: PermissionState,
    /// The params last given to the engine.
    applied: Option<Params>,
    /// The levels last seen, to notice the UI changing them.
    seen_levels: Levels,
    levels_save: Option<PendingLevelsSave>,
    /// When the levels are next written to `levels.json`, if they changed.
    levels_file_save_at: Option<Instant>,
    heartbeat_at: Instant,
    volume_retry_at: Instant,
    auto_level: AutoLevel,
    hearing: Option<HearingSession>,
    shut_down: bool,
    /// The menu and tray visibility last given to the tray.
    tray_state: Option<(Vec<MenuItem>, bool)>,
    /// The icon last given to the tray.
    tray_icon: Option<TrayIconState>,
    ui_requests: Vec<UiRequest>,
}

impl AppModel {
    pub fn new(platform: Platform, store: ProfileStore, launch: Launch, now: Instant) -> AppModel {
        let settings = Settings::load(store.root());
        let levels = Levels::load(store.root());
        let mode = AppMode {
            background: settings.background_processing,
        };
        let engine: SharedEngine =
            Rc::new(RefCell::new(Arc::new(Engine::new(SAMPLE_RATE, MAX_FRAMES))));
        let controller = RouteController::new(platform.audio.route_builder(engine.clone()));
        let mut m = AppModel {
            settings,
            levels,
            editor: Editor::new(store.clone()),
            bypass: mode.bypass_at_start(),
            window_open: !launch.from_login_item,
            current_output: None,
            available_outputs: Vec::new(),
            active_outputs: store.load_active_outputs(),
            assignments: store.load_assignments(),
            other_hosts: Vec::new(),
            testing: false,
            platform,
            store,
            engine,
            controller,
            permission: PermissionState::Unknown,
            applied: None,
            seen_levels: levels,
            levels_save: None,
            levels_file_save_at: None,
            heartbeat_at: now,
            volume_retry_at: now + VOLUME_RETRY_INTERVAL,
            auto_level: AutoLevel::new(now),
            hearing: None,
            shut_down: false,
            tray_state: None,
            tray_icon: None,
            ui_requests: Vec::new(),
        };
        m.update_dock_icon();
        m.refresh_outputs();
        m.restore_pending_volumes(None);
        m.apply_params();
        m.store.prune_heartbeats(SystemTime::now());
        m.heartbeat(now);
        m.sync_inputs(now);
        m
    }

    pub fn tick(&mut self, now: Instant) {
        if self.shut_down {
            return;
        }
        self.editor.tick(now);
        self.notice_level_changes(now);
        for e in self.platform.audio.poll_events() {
            match e {
                PlatformEvent::OutputsChanged => {
                    self.refresh_outputs();
                    self.with_hearing(|h, env| h.check_environment(env));
                    // A device that left mid-test may be back: give it its volume.
                    self.restore_pending_volumes(self.held_volume_uid().as_deref());
                }
                PlatformEvent::SampleRateChanged => self.controller.invalidate(now),
                PlatformEvent::RenderFailed { route_id } => {
                    let ev = self.controller.plugin_died(Some(route_id), now);
                    self.handle(ev, now);
                }
                // A stall from a route that was already replaced means nothing.
                PlatformEvent::RouteStalled { route_id } => {
                    if self.controller.route_id() == Some(route_id) {
                        self.controller.invalidate(now);
                    }
                }
                // Once capture is seen working, silence means nothing (Swift `checkPermission`).
                PlatformEvent::CaptureLooksDenied => {
                    if self.permission != PermissionState::Granted {
                        self.permission = PermissionState::Denied;
                    }
                }
                PlatformEvent::CaptureWorks => self.permission = PermissionState::Granted,
            }
        }
        self.with_hearing(|h, env| h.tick(env, now));
        self.sync_inputs(now);
        self.tick_auto_level(now);
        if now >= self.volume_retry_at {
            self.volume_retry_at = now + VOLUME_RETRY_INTERVAL;
            // Retries a restore that failed while a reconnected device was not ready yet.
            self.restore_pending_volumes(self.held_volume_uid().as_deref());
        }
        let ev = self.controller.tick(now);
        self.handle(ev, now);
        if self.levels_save.as_ref().is_some_and(|p| now >= p.at) {
            self.flush_levels_save();
        }
        if self.levels_file_save_at.is_some_and(|at| now >= at) {
            self.flush_levels_file();
        }
        if now >= self.heartbeat_at {
            let before = self.other_hosts.clone();
            self.heartbeat(now);
            if self.other_hosts != before {
                self.with_hearing(|h, env| h.check_environment(env));
                self.sync_inputs(now);
            }
        }
        self.apply_params();
        for a in self.platform.tray.poll_actions() {
            self.handle_menu(a, now);
        }
        self.update_tray();
    }

    /// Applies a click on a menu bar item. `OpenWindow` and `OpenSettings` are left for the UI
    /// (`take_ui_requests`).
    pub fn handle_menu(&mut self, a: MenuAction, now: Instant) {
        match a {
            MenuAction::ToggleEnabled => self.set_enabled(!self.settings.enabled, now),
            MenuAction::OpenCaptureSettings => self.platform.system.open_capture_settings(),
            MenuAction::LoadProfile(id) => self.load_profile(id, now),
            MenuAction::ToggleOutput(uid) => {
                let devices = self.active_outputs.menu_devices(&self.available_outputs);
                if let Some(d) = devices.into_iter().find(|d| d.uid == uid) {
                    let on = !self.active_outputs.is_checked(&uid);
                    self.set_output_enabled(&d, on, now);
                }
            }
            MenuAction::AssignProfile { uid, profile } => {
                let devices = self.active_outputs.menu_devices(&self.available_outputs);
                if let Some(d) = devices.into_iter().find(|d| d.uid == uid) {
                    self.assign(&d, profile);
                }
            }
            MenuAction::OpenWindow => self.ui_requests.push(UiRequest::OpenWindow),
            MenuAction::OpenSettings => self.ui_requests.push(UiRequest::OpenSettings),
            MenuAction::Quit => self.quit(),
        }
    }

    /// The window requests the menu made since the last call.
    pub fn take_ui_requests(&mut self) -> Vec<UiRequest> {
        std::mem::take(&mut self.ui_requests)
    }

    pub fn is_shut_down(&self) -> bool {
        self.shut_down
    }

    /// The display name of another host app, from its bundle ID.
    pub fn app_name(&self, bundle_id: &str) -> String {
        self.platform.system.app_name(bundle_id)
    }

    /// Gives the tray the menu, visibility and icon when they changed.
    fn update_tray(&mut self) {
        let icon = tray_icon_for(self.route_state(), self.settings.enabled, self.testing);
        if self.tray_icon != Some(icon) {
            self.platform.tray.set_icon(icon);
            self.tray_icon = Some(icon);
        }
        let state = (build_menu(self), self.settings.show_in_menu_bar);
        if self.tray_state.as_ref() == Some(&state) {
            return;
        }
        self.platform.tray.set_menu(&state.0);
        self.platform.tray.set_visible(state.1);
        self.tray_state = Some(state);
    }

    pub fn next_deadline(&self) -> Option<Instant> {
        if self.shut_down {
            return None;
        }
        [
            self.controller.next_deadline(),
            self.editor.next_deadline(),
            self.levels_save.as_ref().map(|p| p.at),
            self.levels_file_save_at,
            Some(self.heartbeat_at),
            Some(self.volume_retry_at),
            self.auto_level.next_deadline(),
            self.hearing
                .as_ref()
                .and_then(HearingSession::next_deadline),
        ]
        .into_iter()
        .flatten()
        .min()
    }

    pub fn route_state(&self) -> &RouteTarget {
        self.controller.state()
    }

    pub fn mode(&self) -> AppMode {
        AppMode {
            background: self.settings.background_processing,
        }
    }

    /// Turning AudioCurve on again retries a denied permission and a failed engine.
    pub fn set_enabled(&mut self, on: bool, now: Instant) {
        self.settings.enabled = on;
        self.settings.save(self.store.root());
        if on {
            self.permission = PermissionState::Unknown;
            if *self.route_state() == RouteTarget::Failed(RouteFailure::Plugin) {
                let ev = self.controller.retry_plugin(now);
                self.handle(ev, now);
            }
        }
        self.sync_inputs(now);
        self.apply_params();
    }

    pub fn set_bypass(&mut self, on: bool, _now: Instant) {
        self.bypass = on;
        self.apply_params();
    }

    /// Applies the new `AppMode`: Bypass as at launch in that mode, no launch at login outside
    /// background mode, and the Dock icon.
    pub fn set_background_processing(&mut self, on: bool) {
        if self.settings.background_processing == on {
            return;
        }
        self.settings.background_processing = on;
        self.settings.save(self.store.root());
        let mode = self.mode();
        self.bypass = mode.bypass_at_start();
        if !mode.offers_launch_at_login() && self.launch_at_login() {
            self.set_launch_at_login(false);
        }
        self.update_dock_icon();
        self.apply_params();
    }

    /// The main window was shown or hidden: the Dock icon follows (`AppMode::shows_dock_icon`).
    pub fn set_window_open(&mut self, open: bool) {
        self.window_open = open;
        self.update_dock_icon();
    }

    /// True once, the first time the background app is running (Swift
    /// `askLaunchAtLoginOnce`): the UI then asks whether to start at login. Remembered in
    /// `settings.json`.
    pub fn take_launch_at_login_question(&mut self) -> bool {
        if !self.mode().offers_launch_at_login() || self.settings.asked_launch_at_login {
            return false;
        }
        self.settings.asked_launch_at_login = true;
        self.settings.save(self.store.root());
        true
    }

    pub fn set_show_in_menu_bar(&mut self, on: bool) {
        self.settings.show_in_menu_bar = on;
        self.settings.save(self.store.root());
    }

    pub fn launch_at_login(&self) -> bool {
        self.platform.system.launch_at_login()
    }

    pub fn set_launch_at_login(&mut self, on: bool) {
        if let Err(e) = self.platform.system.set_launch_at_login(on) {
            eprintln!(
                "AudioCurve: launch at login {} failed: {e}",
                if on { "register" } else { "unregister" }
            );
        }
    }

    /// Ticks or unticks an output in "Active on" and saves the selection.
    pub fn set_output_enabled(&mut self, d: &OutputDevice, on: bool, now: Instant) {
        self.active_outputs
            .set_enabled(d, on, &self.available_outputs);
        self.store.save_active_outputs(&self.active_outputs);
        self.sync_inputs(now);
        self.apply_params();
    }

    /// Assigns a saved profile to an output (None removes the assignment) and saves. A new
    /// assignment copies the current levels. Assigning to the current output loads it at once.
    pub fn assign(&mut self, d: &OutputDevice, profile: Option<Uuid>) {
        self.flush_levels_save();
        let mut a = self.assignments.clone();
        match profile {
            Some(id) => {
                let levels = a
                    .assignment(&d.uid)
                    .and_then(|x| x.levels)
                    .unwrap_or(self.levels.to_settings());
                a.assign(d, Some(id), Some(levels));
            }
            None => {
                a.by_uid.remove(&d.uid);
            }
        }
        self.set_assignments(a);
        if self.current_uid().as_deref() == Some(d.uid.as_str()) {
            self.follow_output();
        }
        self.apply_params();
    }

    pub fn load_profile(&mut self, id: Uuid, _now: Instant) {
        self.editor.load(id);
        self.apply_params();
    }

    /// The "Try Again" of the latched engine failure.
    pub fn retry_engine(&mut self, now: Instant) {
        let ev = self.controller.retry_plugin(now);
        self.handle(ev, now);
    }

    /// Tears the route down first (unmutes at once), then writes pending saves.
    pub fn quit(&mut self) {
        if self.shut_down {
            return;
        }
        self.controller.shut_down();
        // After the route: ending the test clears `testing`, which must not rebuild it.
        self.end_hearing_session();
        self.shut_down = true;
        self.editor.flush_autosave();
        // A change made since the last tick is saved too.
        self.notice_level_changes(Instant::now());
        self.flush_levels_save();
        self.flush_levels_file();
        self.store.remove_heartbeat(std::process::id());
    }

    /// Measures the processed level for 3 s and then sets the output gain (Swift
    /// `startAutoLevel`). Ignored while a measurement runs.
    pub fn start_auto_level(&mut self, now: Instant) {
        self.auto_level.start(now);
    }

    pub fn auto_level(&self) -> &AutoLevelUi {
        &self.auto_level.ui
    }

    /// Restores the volumes a hearing test raised and could not put back, for each device that
    /// is present. A device that is absent keeps its snapshot; `except` is the device a running
    /// test holds at max. What a restore leaves undone stays for the next try, and is given up
    /// after a few (`VolumeSnapshot::remaining`).
    pub fn restore_pending_volumes(&mut self, except: Option<&str>) {
        for s in self.store.pending_volumes() {
            let present = self.available_outputs.iter().any(|o| o.uid == s.device_uid);
            if !present || except == Some(s.device_uid.as_str()) {
                continue;
            }
            let outcomes = self.platform.volume.restore(&s);
            self.store
                .update_pending_volume(&s.device_uid, s.remaining(&outcomes));
        }
    }

    // MARK: Hearing test

    /// Shows the hearing test, with gaps and catch trials from a clock-seeded random source.
    pub fn open_hearing_test(&mut self, now: Instant) {
        self.open_hearing_test_with_random(clock_random(), now);
    }

    /// `open_hearing_test` with the random source given (tests use a fixed one). Does nothing
    /// when a session is open.
    pub fn open_hearing_test_with_random(&mut self, random: Box<dyn FnMut() -> f64>, now: Instant) {
        self.open_hearing_test_with(random, None, now);
    }

    /// `open_hearing_test_with_random`, with `date` as the test date instead of the day the
    /// test starts (screenshots that look the same on every day). Does nothing when a session
    /// is open.
    pub fn open_hearing_test_with(
        &mut self,
        random: Box<dyn FnMut() -> f64>,
        date: Option<SystemTime>,
        now: Instant,
    ) {
        if self.hearing.is_none() {
            self.hearing = Some(HearingSession::new(random, date, now));
            self.with_hearing(|h, env| h.refresh_checks(env));
        }
    }

    /// Back to the main view. Stops a running test first.
    pub fn close_hearing_test(&mut self, now: Instant) {
        self.end_hearing_session();
        self.sync_inputs(now);
    }

    pub fn hearing(&mut self) -> Option<&mut HearingSession> {
        let h = self.hearing.as_mut()?;
        h.observe(self.current_output.as_ref(), &self.other_hosts);
        Some(h)
    }

    /// The display name of the other host that keeps the hearing test from starting, if any.
    pub fn hearing_blocking_host_name(&mut self) -> Option<String> {
        let host = self.hearing()?.blocking_host()?.to_owned();
        Some(self.app_name(&host))
    }

    pub fn hearing_start(&mut self, now: Instant) {
        if self.with_hearing(|h, env| h.may_start(env)) != Some(true) {
            return;
        }
        // An older test's volume first, so the one saved now is the user's.
        self.restore_pending_volumes(None);
        self.with_hearing(|h, env| h.start(env, now));
        self.sync_inputs(now);
    }

    /// "I hear it".
    pub fn hearing_respond(&mut self, now: Instant) {
        self.with_hearing(|h, env| h.respond(env, now));
        self.sync_inputs(now);
    }

    pub fn hearing_pause(&mut self, now: Instant) {
        self.with_hearing(|h, env| h.pause(env, PauseReason::User));
        self.sync_inputs(now);
    }

    pub fn hearing_resume(&mut self, now: Instant) {
        self.with_hearing(|h, env| h.resume(env, now));
        self.sync_inputs(now);
    }

    /// Ends the test (tones off, volume back) and closes the session, except on an output whose
    /// volume the user raised by hand: there a running test goes back to the intro.
    pub fn hearing_cancel(&mut self, now: Instant) {
        if self.with_hearing(|h, env| h.cancel(env)) == Some(true) {
            self.end_hearing_session();
        }
        self.sync_inputs(now);
    }

    /// Leaves the result unsaved.
    pub fn hearing_discard(&mut self, now: Instant) {
        self.close_hearing_test(now);
    }

    /// The main window closed: a running test stops (to the intro, with "the window was
    /// closed"). The window layer calls this on the main window's close event.
    pub fn hearing_window_closed(&mut self, now: Instant) {
        self.with_hearing(|h, env| h.window_closed(env));
        self.sync_inputs(now);
    }

    /// Saves the result as a profile (blank name: the default name), assigns it to the test's
    /// output when `assign`, and closes the session. The loaded profile changes only through the
    /// assignment, and only when the test's output is the current one (Swift `saveMeasured`). On an error the phase becomes `SaveFailed`.
    /// Only in `Done` or `SaveFailed`, where the test no longer holds the route.
    pub fn hearing_save(&mut self, name: &str, assign: bool) {
        let Some(save) = self.hearing.as_mut().and_then(|h| h.measured_save(name)) else {
            return;
        };
        match self
            .editor
            .save_measured(save.id, &save.name, save.profile, save.record)
        {
            Ok(saved) => {
                if assign {
                    self.assign(&save.device, Some(saved.id));
                }
                self.end_hearing_session();
                self.apply_params();
            }
            Err(e) => {
                eprintln!("AudioCurve: saving the hearing test failed: {e}");
                if let Some(h) = self.hearing.as_mut() {
                    h.save_failed();
                }
            }
        }
    }

    fn end_hearing_session(&mut self) {
        self.with_hearing(|h, env| h.shut_down(env));
        self.hearing = None;
    }

    /// The device a running test holds at max.
    fn held_volume_uid(&self) -> Option<String> {
        self.hearing
            .as_ref()
            .and_then(|h| h.held_volume_uid())
            .map(str::to_owned)
    }

    /// Calls `f` with the session and the parts of the app it works with, after giving it the
    /// current output and other hosts.
    fn with_hearing<R>(&mut self, f: impl FnOnce(&mut HearingSession, &mut Env) -> R) -> Option<R> {
        let h = self.hearing.as_mut()?;
        h.observe(self.current_output.as_ref(), &self.other_hosts);
        let mut env = Env {
            tone: &mut *self.platform.tone,
            volume: &mut *self.platform.volume,
            activity: &*self.platform.activity,
            store: &self.store,
            testing: &mut self.testing,
        };
        Some(f(h, &mut env))
    }

    fn tick_auto_level(&mut self, now: Instant) {
        let engine = self.engine();
        if let Some(g) = self
            .auto_level
            .sample(now, || engine.meters(), &self.levels)
        {
            self.levels.output_gain_db = g;
        }
    }

    /// The engine of the running route, for meters and spectrum.
    pub fn engine(&self) -> Arc<Engine> {
        self.engine.borrow().clone()
    }

    /// Processing is on for the current output, AudioCurve is on and Bypass is off.
    pub fn processing_active(&self) -> bool {
        self.active_outputs.is_active(self.current_uid().as_deref())
            && !self.bypass
            && self.settings.enabled
    }

    fn current_uid(&self) -> Option<String> {
        self.current_output.as_ref().map(|d| d.uid.clone())
    }

    fn sync_inputs(&mut self, now: Instant) {
        let inputs = RouteInputs {
            enabled: self.settings.enabled,
            permission: self.permission,
            plugin: PluginLoadState::Loaded,
            output_uid: self.current_uid(),
            active_outputs: self.active_outputs.clone(),
            testing: self.testing,
        };
        self.controller.set_inputs(inputs, now);
    }

    fn handle(&mut self, ev: RouteEvents, now: Instant) {
        if ev.reload_plugin {
            self.reload_engine(now);
        }
    }

    /// A new engine with the current params; the controller then rebuilds the route with it.
    fn reload_engine(&mut self, now: Instant) {
        let p = self.params();
        let e = Engine::new(SAMPLE_RATE, MAX_FRAMES);
        e.set_params(&p);
        *self.engine.borrow_mut() = Arc::new(e);
        self.applied = Some(p);
        self.sync_inputs(now);
    }

    fn params(&self) -> Params {
        self.levels
            .params(self.editor.profile.as_ref(), self.processing_active())
    }

    fn apply_params(&mut self) {
        let p = self.params();
        if self.applied != Some(p) {
            self.engine.borrow().set_params(&p);
            self.applied = Some(p);
        }
    }

    fn update_dock_icon(&mut self) {
        let shown = lifecycle::dock_icon(self.mode(), self.window_open);
        self.platform.system.set_dock_icon(shown);
    }

    fn heartbeat(&mut self, now: Instant) {
        let pid = std::process::id();
        self.store.write_heartbeat(&InstanceHeartbeat {
            pid,
            host_bundle_id: BUNDLE_ID.into(),
            active: self.processing_active(),
            time: SystemTime::now(),
        });
        let live = InstanceHeartbeat::live(&self.store.read_heartbeats(), SystemTime::now(), pid);
        self.other_hosts = live.into_iter().map(|b| b.host_bundle_id).collect();
        self.heartbeat_at = now + InstanceHeartbeat::INTERVAL;
    }

    /// Reads the outputs. Only a real switch of the current output follows it, so a device
    /// list change does not undo a profile the user loaded by hand.
    fn refresh_outputs(&mut self) {
        // Levels still waiting out their delay belong to the output we may be leaving.
        self.flush_levels_save();
        let current = self.platform.audio.current_output();
        let switched =
            current.as_ref().map(|d| &d.uid) != self.current_output.as_ref().map(|d| &d.uid);
        self.current_output = current;
        self.available_outputs = self.platform.audio.outputs();
        if switched {
            self.assignments = self.store.load_assignments();
            self.follow_output();
        }
    }

    /// Loads the current output's profile and levels, saving unsaved edits first (Swift
    /// `AudioCurveAudioUnit.followOutput`).
    fn follow_output(&mut self) {
        let Some(uid) = self.current_uid() else {
            return;
        };
        self.editor.flush_autosave();
        let working = self.editor.profile.clone().map(|profile| WorkingCopy {
            profile,
            source_id: self.editor.source_id,
        });
        let saved_source = self
            .editor
            .source_id
            .and_then(|id| self.store.saved(id))
            .map(|s| s.profile);
        let existing: HashSet<Uuid> = self.store.list().iter().map(|s| s.id).collect();
        let actions = output_switch_actions(
            self.assignments.assignment(&uid),
            working.as_ref(),
            saved_source.as_ref(),
            &existing,
        );
        // If the edits cannot be saved, keep them loaded rather than replace them.
        let mut edits_saved = true;
        for action in actions {
            match action {
                OutputSwitchAction::Overwrite(id, p) => {
                    edits_saved &= self.store.overwrite(id, &p).is_ok()
                }
                OutputSwitchAction::SaveUnsaved(p) => {
                    let date = i18n::formatted_date_time(SystemTime::now(), i18n::lang());
                    let name = format!("{} – {date}", tr("Unsaved"));
                    edits_saved &= self
                        .store
                        .save_new_with(Uuid::new_v4(), &name, &p, None, true)
                        .is_ok();
                }
                OutputSwitchAction::Load(id) => {
                    if edits_saved {
                        self.editor.load(id);
                    }
                }
                OutputSwitchAction::ApplyLevels(l) => {
                    self.levels.apply_settings(&l);
                    self.seen_levels = self.levels;
                }
            }
        }
        self.editor.refresh_saved();
    }

    /// Schedules saving the levels to `levels.json`, and into the current output's assignment
    /// if it has one.
    fn notice_level_changes(&mut self, now: Instant) {
        if self.levels == self.seen_levels {
            return;
        }
        self.seen_levels = self.levels;
        self.levels_file_save_at = Some(now + LEVELS_SAVE_DELAY);
        if let Some(uid) = self
            .current_uid()
            .filter(|u| self.assignments.assignment(u).is_some())
        {
            self.levels_save = Some(PendingLevelsSave {
                uid,
                at: now + LEVELS_SAVE_DELAY,
            });
        }
    }

    /// Writes the levels kept between launches, if a save is pending.
    fn flush_levels_file(&mut self) {
        if self.levels_file_save_at.take().is_some() {
            self.levels.save(self.store.root());
        }
    }

    fn flush_levels_save(&mut self) {
        let Some(pending) = self.levels_save.take() else {
            return;
        };
        let l = self.levels.to_settings();
        let Some(old) = self.assignments.assignment(&pending.uid) else {
            return;
        };
        if old.levels == Some(l) {
            return;
        }
        let mut a = self.assignments.clone();
        a.set_levels(l, &pending.uid);
        self.set_assignments(a);
    }

    /// Prunes references to deleted profiles and saves.
    fn set_assignments(&mut self, mut a: OutputAssignments) {
        a.prune(&self.store.list().iter().map(|s| s.id).collect());
        self.store.save_assignments(&a);
        self.assignments = a;
    }
}
