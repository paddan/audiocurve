# egui App, Stage 2 (macOS Backend) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the Rust app in `desktop/` a real macOS app that processes system audio, lives in the menu bar, and replaces the Swift app in `AudioCurve.app`.

**Architecture:** A `platform::macos` module implements the stage-1 traits (`Audio`, `Volume`, `TestTone`, `Activity`, `System`, `Tray`) with `objc2-core-audio`, `objc2-app-kit`, `objc2-foundation`, `objc2-service-management` and `tray-icon`. It is gated by `#[cfg(target_os = "macos")]`, and the model and UI stay untouched except for the plumbing these tasks name. `main.rs` takes over the app lifecycle: hidden start, Dock icon policy, close-to-hide, reopen, SIGTERM. A small, pure, tested `lifecycle` module makes those decisions. The Makefile and the release workflow then build `AudioCurve.app` from the Rust binary plus the unchanged Swift AUv3, and the Swift `App/` target is deleted.

**Tech Stack:** Rust 2024, eframe/egui 0.37, winit 0.30 (macOS builder extensions), objc2 0.6 family, objc2-core-audio 0.3.2, tray-icon 0.26, signal-hook.

**Spec:** `docs/superpowers/specs/2026-11-18-egui-app-design.md` (stage 2). Stage 1 plan: `docs/superpowers/plans/2026-11-19-egui-app-stage1.md`.

## Global Constraints

- All macOS code lives in `desktop/src/platform/macos/` or behind `#[cfg(target_os = "macos")]` in `main.rs`. `src/model/` and `src/ui/` get no macOS code.
- Behaviour is today's Swift behaviour unless the spec says otherwise. The references are `App/AppModel.swift`, `App/AudioCurveApp.swift`, `App/Audio/*.swift`, and `Packages/AudioCurveCore/Sources/AudioCurveCore/OutputDeviceMonitor.swift`.
- Bundle ID `se.lindefors.audiocurve`. `Info.plist` keeps `LSUIElement`, `NSAudioCaptureUsageDescription` and the version keys, as today.
- The route: a private stereo global process tap excluding this process, with `CATapMuteBehavior::MutedWhenTapped`, then a private aggregate device (UID prefix `se.lindefors.audiocurve.route.`, tap auto-start, drift compensation, the output as main subdevice), then an IOProc running the `Engine`. The buffer is at least 256 frames. Teardown order: stop IO, destroy the aggregate, destroy the tap.
- Every path out of a running route tears it down first, so the Mac is never left muted. That includes quit, SIGTERM, a dropped `BuiltRoute`, and a panic in the IO block (catch it, output silence, report `RenderFailed`).
- The `Engine`'s sample rate always equals the aggregate's nominal rate.
- `TestTone::is_playing` means "the generator is running" (from `start` until `stop`), not "a tone is sounding".
- Hardware-touching tests are `#[cfg(target_os = "macos")]` and run only with `AUDIOCURVE_HW_TESTS=1`. CI never sets it. Any such test that builds a muting route or changes a volume restores everything before it returns, and keeps routes under 1 s.
- Strings: any new user-visible text goes through `i18n::tr` and gets translations in all seven languages (`scripts/merge-xcstrings.py` EXTRA, then regenerate `desktop/strings/Localizable.xcstrings`).
- `cargo fmt`, and `cargo clippy --all-targets -- -D warnings` clean, at the end of every task. Work on a branch `egui-app-stage2`.

## Review Focus

1. The output device disappears while the route runs (headphones unplugged, Bluetooth drops). The route is torn down at once, the system is unmuted, and a new route is built on the new default output. Test in Task 2.
2. The process gets SIGTERM, or panics on the main thread, while the route runs. The tap and aggregate are destroyed before exit. Test in Task 7.
3. The output's sample rate changes (48 → 44.1 kHz) while running. The route and the engine are rebuilt at the new rate. Test in Task 2.
4. A Bluetooth output takes ~1.5 s to start delivering buffers. That startup silence must not be taken as "capture denied". Test in Task 2.
5. Swift's UserDefaults hold a pending volume snapshot in the legacy single-object form, or a corrupt value. Migration restores what it can parse and never crashes. Test in Task 8.

---

### Task 1: HAL helpers, outputs and device events

**Files:**
- Create: `desktop/src/platform/macos/mod.rs`, `desktop/src/platform/macos/hal.rs`, `desktop/src/platform/macos/audio.rs`, `desktop/tests/macos_hal.rs`
- Modify: `desktop/Cargo.toml` (macOS target deps: `objc2`, `objc2-foundation`, `objc2-core-foundation`, `objc2-core-audio`, `objc2-core-audio-types`, `block2`), `desktop/src/platform/mod.rs` (`#[cfg(target_os = "macos")] pub mod macos;`)

**Interfaces:**
- Produces (`platform::macos::hal`, thin safe wrappers, ported from `App/Audio/CoreAudioSupport.swift` and `OutputDeviceMonitor.swift`):
  - `pub type ObjectId = u32`
  - `pub struct HalError { pub status: i32, pub call: &'static str }`
  - `default_output() -> Option<ObjectId>`
  - `device_uid(ObjectId) -> Option<String>`
  - `device_name(ObjectId) -> Option<String>`
  - `device_by_uid(&str) -> Option<ObjectId>`
  - `output_devices() -> Vec<OutputDevice>`: devices with output streams, minus own routes, in HAL order.
  - `is_own_route(uid: &str) -> bool`: prefix `se.lindefors.audiocurve.route.`
  - `nominal_sample_rate(ObjectId) -> Option<f64>`
  - `others_playing() -> bool`: any process object other than ours with `kAudioProcessPropertyIsRunningOutput`.
  - `process_object(pid: i32) -> Option<ObjectId>`
  - `listen(object: ObjectId, selector: u32, f: impl Fn() + Send + 'static) -> Listener`: a block listener on the main dispatch queue, removed when the `Listener` drops.
- Produces (`platform::macos::audio`): `pub struct MacAudio` implementing `Audio`.
  - `outputs` and `current_output` come from `hal`.
  - It listens on the system object for `DefaultOutputDevice` and `Devices`, and pushes `PlatformEvent::OutputsChanged` into an internal `Arc<Mutex<Vec<PlatformEvent>>>` that `poll_events` drains.
  - `route_builder` is filled in by Task 2.
  - `MacAudio::new(waker: Arc<dyn Fn() + Send + Sync>)`: the waker is called after an event is queued, so `main.rs` can request a repaint.

- [ ] **Step 1: Write the failing tests** in `desktop/tests/macos_hal.rs` (`#![cfg(target_os = "macos")]`). These read the HAL only, so they need no gate.
  - `default_output_has_uid_name_and_rate`: `default_output()` is `Some`, its UID and name are non-empty, and its rate is in 8 000..=384 000.
  - `output_devices_contains_the_default_and_no_own_routes`
  - `is_own_route_matches_only_the_prefix`: the Swift test values `se.lindefors.audiocurve.route.1234` gives true and `80-28-3C-24-CB-8C:output` gives false.
  - `device_by_uid_round_trips`
  - `others_playing_returns`: it does not panic, and the value is not asserted.
- [ ] **Step 2:** Run `cd desktop && cargo test --test macos_hal`. Expected: compile errors.
- [ ] **Step 3:** Implement `hal.rs` and `audio.rs`. The spike that proved this path used: `CATapDescription::initStereoGlobalTapButExcludeProcesses` + `setPrivate` + `setMuteBehavior`, `AudioHardwareCreateProcessTap`, an `NSMutableDictionary` toll-free-bridged to `&CFDictionary` for `AudioHardwareCreateAggregateDevice`, `AudioDeviceCreateIOProcIDWithBlock` with a `block2::RcBlock` (kept alive for the IOProc's lifetime) and `None` queue; `AudioObjectGetPropertyData` with `AudioObjectPropertyAddress { mSelector, mScope: kAudioObjectPropertyScopeGlobal, mElement: kAudioObjectPropertyElementMain }`, and CFString UIDs taken with `CFRetained::from_raw`.
- [ ] **Step 4:** Run `cd desktop && cargo test`. Expected: all pass.
- [ ] **Step 5:** Commit `feat(macos): HAL helpers, outputs and device change events`.

### Task 2: The system-wide route

**Files:**
- Create: `desktop/src/platform/macos/route.rs`, `desktop/src/platform/macos/capture_check.rs`, `desktop/tests/macos_route.rs`
- Modify: `desktop/src/engine.rs` (`sample_rate()`, `last_params()`, `with_sample_rate(fs) -> Engine`), `desktop/src/platform/macos/audio.rs` (`route_builder`, render and route events, capture check in `poll_events`)

**Interfaces:**
- Consumes: `hal` (Task 1), `Engine`, `SharedEngine`, core's `RouteBuilder`/`BuiltRoute`.
- Produces:
  - `Engine::sample_rate(&self) -> f64`
  - `Engine::last_params(&self) -> Params` (the engine remembers the last `set_params`)
  - `Engine::with_sample_rate(&self, fs: f64) -> Engine` (a new engine at `fs` carrying `last_params`)
  - `route::MacRouteBuilder { engine: SharedEngine, events: EventQueue, mute: bool }` implementing `RouteBuilder`. `mute` is true in production; tests pass false.
  - `build(uid)` does the following, in order:
    1. Create the tap, excluding `process_object(getpid())`, and the aggregate.
    2. Set the buffer to at least 256 frames.
    3. If `engine.sample_rate() != aggregate rate`, put `engine.with_sample_rate(rate)` into the `SharedEngine`.
    4. Create the IOProc block: it copies the tap input into preallocated L/R buffers, runs `Engine::process` in place, writes the output, records the host time of the last non-zero input sample in an `AtomicU64`, and catches panics (`std::panic::catch_unwind` with `AssertUnwindSafe`), outputting silence and pushing `RenderFailed { route_id }` once.
    5. Add listeners on the output (`NominalSampleRate`, `DeviceIsAlive`), the aggregate (`DeviceIsAlive`) and the system (`ServiceRestarted`), each pushing `SampleRateChanged`. `AppModel` already maps that to `invalidate`.
    6. Start IO.

    It returns `MacRoute`, which implements `BuiltRoute`. `tear_down` stops IO, destroys the IOProc and aggregate, removes the listeners and destroys the tap. `Drop` does the same if `tear_down` was never called.
  - `capture_check::looks_denied(silent_for: Duration, others_playing: bool, since_first_buffer: Duration) -> bool`: true only when `others_playing`, `silent_for >= 3 s` and `since_first_buffer >= 3 s`, so a Bluetooth output that starts slowly never counts. `MacAudio::poll_events` uses it on the running route and pushes `CaptureLooksDenied` once per route.
- [ ] **Step 1: Write the failing tests:**
  - Pure, always run:
    - `looks_denied_needs_others_playing_and_3s_of_silence`
    - `slow_bluetooth_start_is_not_denied` (Review Focus 4: `since_first_buffer` = 1.5 s with 10 s of silence gives false)
    - `engine_with_sample_rate_keeps_params`
  - Gated by `AUDIOCURVE_HW_TESTS=1`, `mute: false`:
    - `route_builds_runs_and_tears_down_on_the_default_output`: the IOProc runs at least 10 callbacks within 3 s. After teardown, no device with the own-route prefix remains in `hal`'s full device list.
    - `dropping_a_route_tears_it_down`
    - `engine_follows_the_aggregate_rate`: start with a 44 100 engine, and after `build`, `SharedEngine`'s rate equals `nominal_sample_rate(aggregate)` (Review Focus 3).
    - Review Focus 1, `device_events_push_sample_rate_changed`: call the listener closure directly via a test hook, `MacRoute::simulate_device_event()`, and assert the queued event.
- [ ] **Step 2:** Run `cd desktop && cargo test --test macos_route` and `AUDIOCURVE_HW_TESTS=1 cargo test --test macos_route`. Expected: compile errors.
- [ ] **Step 3:** Implement, using the calls listed in Task 1 Step 3 and porting `CoreAudioRouteBuilder.swift`, `ProcessTap.swift`, `AggregateDevice.swift`, `RenderLoop.swift`. The IO block must not allocate or lock.
- [ ] **Step 4:** Run both commands from Step 2. Expected: all pass, and the hardware run leaves the Mac's sound unchanged.
- [ ] **Step 5:** Commit `feat(macos): system-wide route through the engine`.

### Task 3: Device volume

**Files:**
- Create: `desktop/src/platform/macos/volume.rs`, `desktop/tests/macos_volume.rs`

**Interfaces:**
- Produces: `MacVolume` implementing `Volume`, ported from `App/Audio/DeviceVolume.swift`. The elements are main, 1 and 2. A volume at or above `0.99` counts as max. `restore` reports `ElementRestore::{Restored, NotWritable, Failed}` per element. `is_muted` reads `kAudioDevicePropertyMute` on the main element, then on elements 1 and 2. `is_settable` asks `AudioObjectIsPropertySettable` on any element.
- [ ] **Step 1: Write the failing tests**, gated by `AUDIOCURVE_HW_TESTS=1`:
  - `snapshot_set_max_and_restore_round_trip`: take a snapshot, `set_max`, read it back (every settable element is at least 0.99), `restore`, and check the scalars equal the snapshot within 0.001. Restore the original volume even if an assertion fails, using a drop guard.
  - `is_muted_and_is_settable_answer`
- [ ] **Step 2:** Run `AUDIOCURVE_HW_TESTS=1 cargo test --test macos_volume`. Expected: compile errors.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Run the command from Step 2. Expected: pass, with the volume unchanged afterwards.
- [ ] **Step 5:** Commit `feat(macos): device volume`.

### Task 4: Test tone output

**Files:**
- Create: `desktop/src/platform/macos/tone.rs`, `desktop/tests/macos_tone.rs`

**Interfaces:**
- Produces: `MacTestTone` implementing `TestTone`.
  - `start(uid)` creates an IOProc directly on that output device (no aggregate) and starts it. The IOProc renders core's `ToneSynth` into the device's first two channels and writes silence to the rest. The synth is shared with the IO block lock-free: a single-producer command slot (`AtomicU64` sequence plus fixed fields), as `App/Audio/ToneGenerator.swift` does.
  - `play` posts the next tone.
  - `is_playing` is true from `start` until `stop`, and also goes false if the device stops (`DeviceIsAlive` or `DeviceIsRunningSomewhere` listener).
  - `sample_rate(uid)` is `hal::nominal_sample_rate`.
  - `stop` stops and destroys the IOProc.
  - Frequency range is 125–8000 Hz. Below 22 050 Hz, `start` returns `Err`, as Swift's `minimumSampleRate` does.
- [ ] **Step 1: Write the failing tests:**
  - `tone_rejects_low_sample_rate`: pure, through a `check_rate(f64) -> Result<(), String>` helper.
  - Gated: `start_play_silent_and_stop_on_default_output`: `start`, then `is_playing`, then `play(Left, 1000, None)` (silent), wait 1.2 s, `stop`, then `!is_playing`.
- [ ] **Step 2:** Run the tests. Expected: compile errors.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Run the tests. Expected: pass.
- [ ] **Step 5:** Commit `feat(macos): hearing test tone output`.

### Task 5: Activity, system services and the menu bar

**Files:**
- Create: `desktop/src/platform/macos/system.rs`, `desktop/src/platform/macos/tray.rs`, `desktop/tests/macos_system.rs`
- Modify: `desktop/Cargo.toml` (`objc2-app-kit`, `objc2-service-management`, `tray-icon`)

**Interfaces:**
- Produces:
  - `MacActivity` implementing `Activity` (`hal::others_playing`).
  - `MacSystem` implementing `System`:
    - `launch_at_login` and `set_launch_at_login` use `SMAppService.mainApp`.
    - `set_dock_icon` sets `NSApplicationActivationPolicy::Regular` or `Accessory`.
    - `open_capture_settings` opens `x-apple.systempreferences:com.apple.settings.PrivacySecurity.extension?Privacy_AudioCapture`.
    - `app_name(bundle_id)` uses the NSWorkspace application URL, then the NSFileManager display name without ".app", and falls back to `tr("another app")`.
  - `MacTray` implementing `Tray` with `tray-icon`:
    - The menu-bar item has a template-image ear icon. Use `App/MenuBarIcon.swift`'s drawing as reference; draw it as an RGBA buffer, or use the SF Symbol `ear` via `NSImage`.
    - `set_menu` rebuilds the `muda` menu from `[MenuItem]` and records `MenuId → MenuAction` in a map.
    - A `MenuEvent` handler sends actions over a channel and calls the waker.
    - `set_visible` shows or hides the item.
  - The pure helper `tray::flatten(items: &[MenuItem]) -> Vec<(String /*path*/, Option<MenuAction>)>` is used by the mapping and by tests.
- [ ] **Step 1: Write the failing tests** in `desktop/tests/macos_system.rs`:
  - `app_name_of_finder_is_finder`
  - `unknown_bundle_id_falls_back`
  - `flatten_maps_every_action_once`: built with the fake platform's demo model through `build_menu`.
- [ ] **Step 2:** Run the tests. Expected: compile errors.
- [ ] **Step 3:** Implement. Create the tray on the main thread after the event loop starts, as the lifecycle spike did. Call `TrayIconBuilder` from `main.rs`'s creation callback (Task 7) through `MacTray::new(waker)`.
- [ ] **Step 4:** Run the tests. Expected: pass.
- [ ] **Step 5:** Commit `feat(macos): activity, system services and menu bar`.

### Task 6: The macOS platform

**Files:**
- Modify: `desktop/src/platform/macos/mod.rs`

**Interfaces:**
- Produces: `pub fn mac_platform(waker: Arc<dyn Fn() + Send + Sync>) -> Platform`, which assembles `MacAudio`, `MacVolume`, `MacTestTone`, `MacActivity`, `MacSystem` and `MacTray`.
- [ ] **Step 1: Write the failing test** `mac_platform_assembles_and_lists_outputs`. It is not gated: `mac_platform(..).audio.outputs()` is non-empty and `current_output()` is `Some`. Building the platform must not build a route or touch volume.
- [ ] **Step 2:** Run it. Expected: compile error.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Run it. Expected: pass.
- [ ] **Step 5:** Commit `feat(macos): assemble the platform`.

### Task 7: App lifecycle

**Files:**
- Create: `desktop/src/lifecycle.rs`, `desktop/tests/lifecycle.rs`
- Modify: `desktop/src/main.rs`, `desktop/Cargo.toml` (`winit` 0.30 for the macOS builder extensions, `signal-hook`), `desktop/src/platform/macos/route.rs` (register the running route's teardown in `ROUTE_GUARD`, clear it on teardown), `desktop/src/model/app.rs` (`launch` accessors only if needed)

**Interfaces:**
- Produces (pure, in `lifecycle`):
  - `pub enum CloseAction { Hide, Quit }`
  - `pub fn on_close_request(mode: AppMode) -> CloseAction`: background gives Hide, foreground gives Quit.
  - `pub fn starts_visible(from_login_item: bool) -> bool`
  - `pub fn should_reopen(window_visible: bool, became_active: bool, launch_settled: bool) -> bool`: true only when the window is hidden, the app just became active, and more than 1 s has passed since launch.
  - `pub fn dock_icon(mode: AppMode, window_open: bool) -> bool` (`AppMode::shows_dock_icon`).
- `main.rs` on macOS:
  - Platform: `mac_platform(waker)`, where the waker calls `egui::Context::request_repaint`.
  - Launch: `from_login_item` comes from `NSAppleEventManager.currentAppleEvent`'s `keyAEPropData == keyAELaunchedAsLogInItem`, as `AudioCurveApp.launchedAsLoginItem` does. Read it in the creation callback. If it is not available there, record it in the `event_loop_builder` hook and say so in the report.
  - A hidden start uses `ActivationPolicy::Accessory` and `with_activate_ignoring_other_apps(false)` through `NativeOptions::event_loop_builder`, as the spike showed.
  - Close request: `CancelClose`, then `on_close_request`. Hide gives `hearing_window_closed`, `Visible(false)` and the Dock icon off. Quit gives `hearing_window_closed`, `quit()` and `Close`.
  - Reopen: `NSApplication.isActive` rising edge, through `should_reopen`, shows and focuses the window and turns the Dock icon on.
  - Ticking while hidden: `request_repaint_after(next_deadline)`, at least once a second.
  - SIGTERM, SIGINT and SIGHUP: a `signal-hook` flag, checked every tick (and the waker is called from a signal-hook thread), leads to `quit()` and then `Close`.
  - A panic hook, before aborting, tears down via a global `Weak` to the platform's route. Use a `static ROUTE_GUARD: Mutex<Option<Box<dyn FnOnce() + Send>>>` that `MacRouteBuilder` fills.
  - Launch-at-login is asked once: with `AppMode::offers_launch_at_login` and a `settings.json` flag `asked_launch_at_login`, show an egui modal with the Swift text and the "Start at Login" and "Not Now" buttons (check the catalog keys).
  - ⌘, opens Settings.
  - Off macOS, the fake platform stays as today.
- [ ] **Step 1: Write the failing tests** in `desktop/tests/lifecycle.rs` for the four pure functions, using the Swift `AppMode` rules. Also Review Focus 2, gated: `sigterm_child_leaves_no_route`. It spawns the built binary with env `AUDIOCURVE_TEST_ROUTE_THEN_WAIT=1`, a test-only path that builds an unmuted route on the default output and then waits. It sends SIGTERM after 1 s, then asserts there are no own-route devices in the HAL device list and that the child's exit status is success.
- [ ] **Step 2:** Run the tests. Expected: compile errors.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Run the tests, including the gated one. Then `cargo run --release` by hand: the window opens, and the status shows "Active – <your output>" (or the permission prompt appears). Expected: tests pass and the app runs.
- [ ] **Step 5:** Commit `feat(macos): app lifecycle, signals and launch at login`.

### Task 8: Migrating the Swift app's UserDefaults

**Files:**
- Create: `desktop/src/platform/macos/migrate.rs`, `desktop/src/migrate_swift.rs` (the pure parser), `desktop/tests/migrate_swift.rs`
- Modify: `desktop/src/main.rs` (run once at startup, before `AppModel::new`)

**Interfaces:**
- Produces (pure, cross-platform):
  - `pub fn parse_pending_volumes(json: &[u8]) -> Vec<VolumeSnapshot>`. It accepts Swift's encoding: an array, or a single legacy object, of `{"deviceUID": String, "scalars": [k0, v0, k1, v1, …], "failedAttempts": Int?}`. Anything else gives an empty vec.
  - `pub struct SwiftDefaults { pub enabled: Option<bool>, pub background_processing: Option<bool>, pub show_in_menu_bar: Option<bool>, pub pending_volume: Option<Vec<u8>> }`
  - `pub fn migrate(d: &SwiftDefaults, store: &ProfileStore, settings_root: &Path) -> bool` merges the snapshots into the store with core's `save_pending_volume` (an existing original is never replaced). It writes the three settings into `settings.json` only if `settings.json` does not exist yet, and returns whether the pending key should be removed.
- macOS side: reads `NSUserDefaults::standardUserDefaults`. Bundled, that is the `se.lindefors.audiocurve` domain. Unbundled `cargo run`, use `initWithSuiteName("se.lindefors.audiocurve")`. Keys: `enabled`, `backgroundProcessing`, `showInMenuBar`, `hearingTest.pendingVolume`. It removes `hearingTest.pendingVolume` after a successful migration and writes a `migrated-swift-defaults` marker in the store root so the migration runs once.
- [ ] **Step 1: Write the failing tests** in `desktop/tests/migrate_swift.rs`:
  - `parses_swift_array_form` (`[{"deviceUID":"x","scalars":[0,0.5,1,0.25],"failedAttempts":2}]`)
  - `parses_legacy_single_object` (Review Focus 5)
  - `corrupt_json_gives_nothing`
  - `snapshot_without_attempts_parses_with_zero`
  - `migrate_keeps_existing_original_and_fills_settings_once`
- [ ] **Step 2:** Run the tests. Expected: compile errors.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Run the tests. Expected: pass.
- [ ] **Step 5:** Commit `feat(macos): migrate the Swift app's settings and pending volumes`.

### Task 9: Packaging, release, removing the Swift app, screenshots

**Files:**
- Create: `desktop/macos/Info.plist` (moved from `App/Info.plist`), `scripts/check-bundle.sh`
- Modify: `Makefile`, `.github/workflows/release.yml`, `project.yml`, `README.md`, `docs/manual-test.md`, `docs/screenshots/*`
- Delete: `App/*.swift`, `App/Audio/*.swift`, `scripts/screenshots/WindowSnapshot.swift`, `App/Localizable.xcstrings` (after confirming that `desktop/strings` covers its keys), `docs/screenshots/panel-*.png`

**Interfaces:**
- Makefile:
  - `$(OBJ_DIR)/%/$(APP_NAME)` is built by `cd desktop && cargo build --release --locked --target <triple>`, copying `desktop/target/<triple>/release/audiocurve`.
  - The bundle step uses `desktop/macos/Info.plist` with `CFBundleExecutable` = `AudioCurve`, generates `InfoPlist.strings` per language for `NSAudioCaptureUsageDescription` from `desktop/strings` (extend `scripts/xcstrings2strings.py` or add a sibling script), and keeps the icon, the AUv3 appex build, signing and `install`/`uninstall` unchanged.
  - `make screenshots` now runs the egui screenshots into `docs/screenshots/` with the README's file names: `audiogram`, `gain`, `spectrum`, `hearingtest-running` and `hearingtest-result`, each `-light`/`-dark`. Map them from the `render_all` names. `make egui-screenshots` is removed.
- `project.yml`: remove the `AudioCurve` app target and its embedding. Keep the settings block that holds `MARKETING_VERSION`/`CURRENT_PROJECT_VERSION`, which the Makefile reads, and add a comment that `make` is the build path.
- `release.yml`: replace `xcodegen`/`xcodebuild` with `make release ARCHS=arm64 MARKETING_VERSION=… CURRENT_PROJECT_VERSION=…` (command-line variables already override the Makefile's `:=` values read from `project.yml`), drop `brew install xcodegen`, copy `build/release/AudioCurve.app` to `build/out/`, and add `(cd desktop && cargo test)` to Test.
- README: replace the panel screenshot row with a line that says the menu is a native menu, and keep the other images.
- `docs/manual-test.md`: a new section, "Stage 2 – Rust app", with the spec's manual checklist:
  - processing on and off
  - output switching
  - sleep and wake
  - unplugging a device mid-test
  - a crash mid-test (`kill -9`, then relaunch: the volume is restored)
  - permission denied and granted
  - the login item
  - both background modes
  - all eight languages render
  - the AUv3 still loads in a host
- `scripts/check-bundle.sh APP`: checks that `Contents/MacOS/AudioCurve` exists and is arm64, that `Info.plist` has the bundle ID, `LSUIElement` and `NSAudioCaptureUsageDescription`, that `PlugIns/AudioCurveAU.appex` exists, and runs `codesign --verify --deep --strict`.
- [ ] **Step 1:** Write `scripts/check-bundle.sh`, and run it against today's Swift-built bundle (`make release && scripts/check-bundle.sh build/release/AudioCurve.app`). Expected: it passes on the Swift build, so the check itself is right.
- [ ] **Step 2:** Change the Makefile, `Info.plist`, `project.yml` and the scripts. Run `make clean release && scripts/check-bundle.sh build/release/AudioCurve.app`. Expected: pass, with `Contents/MacOS/AudioCurve` now the Rust binary (`otool -L` shows no Swift app libraries).
- [ ] **Step 3:** Delete the Swift app sources and the old screenshot tool, then run `make test` and `make screenshots`. Expected: the tests pass (the Swift package tests still run for the AUv3), and the README images are regenerated.
- [ ] **Step 4:** Update `release.yml`, the README and `manual-test.md`. Run `make release` once more and `make install`, then open `/Applications/AudioCurve.app`. Expected: the Rust app starts from the bundle, the system audio prompt names AudioCurve, and the AUv3 is registered (`pluginkit -m -i se.lindefors.audiocurve.AudioCurveAU`).
- [ ] **Step 5:** Commit `build: ship the Rust app in AudioCurve.app and remove the Swift app`.
