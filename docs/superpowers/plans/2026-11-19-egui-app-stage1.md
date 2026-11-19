# egui App, Stage 1 (UI with a Fake Platform) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A Rust crate `desktop/` with the whole AudioCurve app (model and egui UI) running on macOS against a fake platform, so it can be used, tested and screenshotted before the macOS backend exists.

**Architecture:** `desktop/` is a standalone crate (binary `audiocurve`, library `audiocurve_app`) depending on `../core` and `../dsp`. `src/model/` holds the app logic, ported from today's `App/AppModel.swift`, `AU/PluginViewModel.swift`, the app-facing parts of `AU/AudioCurveAudioUnit.swift`, and `App/HearingTestModel.swift`. It contains no platform code and is driven by `tick(now)` like core's route controller. `src/platform/` holds the platform traits and their `fake` implementation; the `macos` implementation is stage 2. `src/ui/` holds egui views that only read and call the model.

**Tech Stack:** Rust 2024; `eframe`/`egui` 0.37 (wgpu); `egui_kittest` 0.37 (wgpu, snapshot) for UI tests and screenshots; `sys-locale` for the system language; `serde_json` (build script) for the strings; `audiocurve-core`, `audiocurve-dsp`.

**Spec:** `docs/superpowers/specs/2026-11-18-egui-app-design.md` (stage 1 only). The core it builds on: `docs/superpowers/specs/2026-11-18-rust-core-design.md`.

## Global Constraints

- No macOS code in `src/model/` or `src/ui/`, and no `#[cfg(target_os)]` there. Everything platform-specific goes through the traits in `src/platform/mod.rs`.
- Stage 1 changes no Swift code and does not touch the release build. The Swift app keeps shipping. The README and `make screenshots` stay as they are. The egui screenshots go to a new target, `make egui-screenshots`.
- Behaviour is today's Swift behaviour unless the spec says otherwise. For any rule, value or text, the reference is the Swift file named in the task.
- Strings: every user-visible text goes through `i18n::tr`, keyed by today's English source string. The eight languages are en, sv, da, nb, fi, de, fr, es. A new or changed string gets translations in all seven non-English languages in the same task.
- The tuned theme, verbatim from the spec:
  - proportional font SF (`/System/Library/Fonts/SFNS.ttf` on macOS, egui's default elsewhere); Body and Button text 14 pt
  - button padding 10×4, item spacing 8×6, interact height 24
  - accent `#0A6EF5` for selection fill (white selection stroke) and links
  - `slider_trailing_fill` on
  - rounding 7 on all widget states, 12 on windows and on the controls panel frame (inner margin 14); no border on inactive widgets
  - plot colours: left `#467DD7`, right `#D25A5A`, normal-hearing band green at ~11 % opacity
  - segmented control: a 7 pt rounded track in the faint background colour, the selected segment filled with the accent and white text
- Light and dark follow the system appearance. eframe's default theme preference is "follow system", so leave it.
- AccessKit stays on. Every `egui_kittest::Harness` keeps its default accessibility check (no `with_accessibility_check(false)`), so every widget needs an accessible name.
- Engine failure is the route controller's "plugin died". Reloading means a new `Engine` with the current params, after which the route is rebuilt. The latched failure shows the new status text "The sound processing stopped" with a "Try Again" button.
- Left to stage 2 (macOS backend, its own plan): the CoreAudio/AppKit implementations of the traits, the real tray, the Dock icon and reopen handling, launch at login, the one-time migration of the Swift app's UserDefaults, packaging, replacing `make screenshots`/README images, and deleting the Swift `App/` target.
- Work on a branch `egui-app`, not on `master`. `cargo clippy --all-targets -- -D warnings` must be clean at the end of every task.

## Review Focus

1. Output switch while the editor has unsaved edits: the old source profile is overwritten (or the unsaved profile saved) before the new output's profile loads, and no edit is lost. Test in Task 6.
2. The output changes while a hearing test runs: the test stops with `StopReason::OutputChanged`, the tone stops, and the raised volume is restored. Test in Task 8.
3. Two engine failures within 10 s: the status shows "The sound processing stopped", and "Try Again" rebuilds the engine and route. Test in Task 6.
4. A typed audiogram value that is out of range or not a number (Enter on "abc" or "130"): it is ignored, the buffer is cleared, and the profile is unchanged. Test in Task 5.
5. A very small window (the minimum 600×420) or a plot of zero height: no panic and no NaN geometry. Test in Task 10.

---

### Task 1: Crate, platform traits, engine and fake platform

**Files:**
- Create: `desktop/Cargo.toml`, `desktop/src/lib.rs`, `desktop/src/engine.rs`, `desktop/src/platform/mod.rs`, `desktop/src/platform/fake.rs`, `desktop/tests/fake_platform.rs`
- Modify: `Makefile` (target `test`: add `cd desktop && "$(CARGO)" test`), `.gitignore` (`desktop/target/`)

**Interfaces:**
- Produces (`engine`): `pub struct Engine` wrapping `Box<audiocurve_dsp::Handle>`; `Engine::new(sample_rate: f64, max_frames: usize) -> Engine` (panics on invalid args; the callers pass 48 000 / 4096); `set_params(&self, &audiocurve_dsp::Params)`, `meters(&self) -> Meters`, `spectrum(&self) -> Spectrum`, `unsafe fn process(&self, l: &mut [f32], r: &mut [f32])`. `pub type SharedEngine = Rc<RefCell<Arc<Engine>>>`: the model swaps the `Arc` on reload, and a route builder clones the current `Arc` at build time.
- Produces (`platform`):
  - `pub enum PlatformEvent { OutputsChanged, SampleRateChanged, RenderFailed { route_id: Uuid }, CaptureLooksDenied }`
  - `pub trait Audio { fn outputs(&self) -> Vec<OutputDevice>; fn current_output(&self) -> Option<OutputDevice>; fn route_builder(&self, engine: SharedEngine) -> Box<dyn RouteBuilder>; fn poll_events(&mut self) -> Vec<PlatformEvent>; }`
  - `pub trait Volume { fn snapshot(&self, uid: &str) -> Option<VolumeSnapshot>; fn set_max(&mut self, uid: &str) -> bool; fn restore(&mut self, s: &VolumeSnapshot) -> BTreeMap<u32, ElementRestore>; fn is_muted(&self, uid: &str) -> bool; fn is_settable(&self, uid: &str) -> bool; }`
  - `pub trait TestTone { fn start(&mut self, uid: &str) -> Result<(), String>; fn play(&mut self, ear: Ear, frequency_hz: f64, level_dbfs: Option<f64>); fn is_playing(&self) -> bool; fn stop(&mut self); fn sample_rate(&self, uid: &str) -> f64; }` The real one renders core's `ToneSynth`.
  - `pub trait Activity { fn others_playing(&self) -> bool; }`
  - `pub trait System { fn launch_at_login(&self) -> bool; fn set_launch_at_login(&mut self, on: bool) -> Result<(), String>; fn set_dock_icon(&mut self, shown: bool); fn open_capture_settings(&mut self); fn app_name(&self, bundle_id: &str) -> String; }`
  - `pub struct Platform { pub audio: Box<dyn Audio>, pub volume: Box<dyn Volume>, pub tone: Box<dyn TestTone>, pub activity: Box<dyn Activity>, pub system: Box<dyn System> }`
- Produces (`platform::fake`): `pub fn fake() -> (Platform, FakeControl)`. `FakeControl` is a clonable handle (`Rc<RefCell<FakeState>>`) that tests use to steer the fake platform: `set_outputs(Vec<OutputDevice>, current: Option<&str>)`, `fail_next_builds(n)`, `emit(PlatformEvent)`, `set_muted(uid, bool)`, `set_others_playing(bool)`, `volume_log() -> Vec<String>` (`"max:<uid>"`, `"restore:<uid>"`), `tone_log() -> Vec<String>`, `route_log() -> Vec<String>` (`"build:<uid>"`, `"teardown:<uid>"`), `pump(frames: usize)`. `pump` runs generated pink noise at -20 dBFS through the engine of the running route, synchronously. The fake's default outputs are `AirPods Pro` (uid `airpods`, current), `Hearing Aids` (`aids`) and `Mac Studio Speakers` (`speakers`), the same made-up names as the README. A fake route also starts a background thread that pumps noise in real time while `!cfg!(test)` and the binary is running. The thread stops on teardown.

- [ ] **Step 1:** Create branch `egui-app`. Create `desktop/Cargo.toml`: `name = "audiocurve-app"`, `[lib] name = "audiocurve_app"`, `[[bin]] name = "audiocurve" path = "src/main.rs"`, edition 2024. Dependencies: `audiocurve-core = { path = "../core" }`, `audiocurve-dsp = { path = "../dsp" }`, `eframe = { version = "0.37", default-features = false, features = ["wgpu", "accesskit", "default_fonts"] }`, `egui = "0.37"`, `sys-locale`, `uuid`, `serde_json`. Dev-dependencies: `egui_kittest = { version = "0.37", features = ["wgpu", "snapshot"] }`, `tempfile`. Add a placeholder `src/main.rs` with `fn main() {}`; Task 13 replaces it.
- [ ] **Step 2: Write the failing tests** in `desktop/tests/fake_platform.rs`:
  - `fake_lists_the_demo_outputs`: three outputs, current `airpods`.
  - `fake_route_builds_tears_down_and_fails_on_request`: `route_builder(engine).build("airpods")` logs `build:airpods`; `tear_down` logs `teardown:airpods`; after `fail_next_builds(1)` the next build is `Err`.
  - `pumping_a_running_route_moves_the_meters`: build with an `Engine` whose params are `Params::unity()`, `pump(48_000)`, then `engine.meters().in_rms_db[0] > -40.0` and `out_rms_db[0] > -40.0`.
  - `emitted_events_are_polled_once`: `emit(OutputsChanged)`, then `poll_events() == [OutputsChanged]`, then `poll_events()` is empty.
- [ ] **Step 3:** Run `cd desktop && cargo test --test fake_platform`. Expected: compile errors.
- [ ] **Step 4:** Implement `engine.rs`, `platform/mod.rs` and `platform/fake.rs`.
- [ ] **Step 5:** Run `cd desktop && cargo test` and `make test`. Expected: all pass.
- [ ] **Step 6:** Commit `feat(app): crate, platform traits, engine and fake platform`.

### Task 2: Strings in eight languages

**Files:**
- Create: `desktop/strings/Localizable.xcstrings`, `desktop/build.rs`, `desktop/src/i18n.rs`, `desktop/tests/i18n.rs`, `scripts/merge-xcstrings.py`

**Interfaces:**
- Produces (`i18n`): `pub enum Lang { En, Sv, Da, Nb, Fi, De, Fr, Es }`; `Lang::from_locale(tag: &str) -> Lang` (matches on the language subtag; `nb` and `no` give `Nb`; anything else gives `En`); `pub fn set_lang(Lang)`; `pub fn lang() -> Lang` (a process-wide setting, `En` by default); `pub fn tr(source: &str) -> String`; `pub fn trf(source: &str, args: &[&str]) -> String`. `trf` replaces `%@` (and `%1$@`, `%2$@` …) in the translation with `args` in order. A missing key or language returns the English source, with the same replacement.
- `build.rs` reads `strings/Localizable.xcstrings` (xcstrings JSON) and generates a `phf`-free `match`-based lookup into `OUT_DIR/strings.rs`, included by `i18n.rs`.

- [ ] **Step 1:** Write `scripts/merge-xcstrings.py IN1 IN2 OUT`. It merges `App/Localizable.xcstrings` and `AU/Localizable.xcstrings` into one file in the same format. On a key both files have, it keeps the AU entry. Run it to create `desktop/strings/Localizable.xcstrings`. Then add these keys with translations in all seven languages:
  - "The sound processing stopped"
  - "Try Again"
  - "AudioCurve on"
  - "Settings…" (if missing)
  - every string the plan's later tasks list as new
- [ ] **Step 2: Write the failing tests** in `desktop/tests/i18n.rs`:
  - `swedish_lookup`: `set_lang(Lang::Sv)`; `tr("Active on")` equals the `sv` value of "Active on" in the merged file (read it in the test with `serde_json` from `include_str!("../strings/Localizable.xcstrings")`).
  - `missing_key_falls_back_to_english`: `tr("No such string 123") == "No such string 123"`.
  - `format_arguments`: `set_lang(Lang::En)`; `trf("Profile for %@", &["AirPods Pro"]) == "Profile for AirPods Pro"`.
  - `locale_tags`: `from_locale("sv-SE") == Sv`, `from_locale("nb-NO") == Nb`, `from_locale("no") == Nb`, `from_locale("pt-BR") == En`.
  - `every_key_has_all_eight_languages_or_english_only`: for each key that has any translation, all seven non-English languages are present.

  The tests touch a process-wide setting, so serialize them with a `static LOCK: Mutex<()>`.
- [ ] **Step 3:** Run `cd desktop && cargo test --test i18n`. Expected: compile errors.
- [ ] **Step 4:** Implement `build.rs` and `i18n.rs`.
- [ ] **Step 5:** Run `cd desktop && cargo test`. Expected: all pass.
- [ ] **Step 6:** Commit `feat(app): strings in eight languages from the existing translations`.

### Task 3: Levels, settings and app mode

**Files:**
- Create: `desktop/src/model/mod.rs` (declares the model modules), `desktop/src/model/levels.rs`, `desktop/src/model/settings.rs`, `desktop/tests/levels_settings.rs`

**Interfaces:**
- Consumes: `audiocurve_core::fitting::{ParamsInput, CompressionMode, params}`, `audiocurve_core::outputs::LevelSettings`.
- Produces (`model::levels`): `pub struct Levels { pub strength: f64 /*0..100, default 50*/, pub tone: f64 /*0..100, default 0*/, pub mpo_dbfs: f64 /*default -3*/, pub output_gain_db: f64 /*0*/, pub input_gain_db: f64 /*0*/, pub mode: CompressionMode /*Auto*/, pub compression: bool /*true*/, pub limiter: bool /*true*/ }`. Plus `Default`; `params(&self, profile: Option<&HearingProfile>, active: bool) -> Params` (strength and tone divided by 100); `to_settings(&self) -> LevelSettings` and `apply_settings(&mut self, &LevelSettings)` (the 0…1 ↔ 0…100 conversions as in `AU/PluginViewModel.swift`). Slider ranges, from `AU/Parameters.swift`: `pub const MPO_RANGE`, `OUTPUT_GAIN_RANGE`, `INPUT_GAIN_RANGE: RangeInclusive<f64>`.
- Produces (`model::settings`): `pub struct Settings { pub enabled: bool, pub background_processing: bool, pub show_in_menu_bar: bool }` (all default true), stored as `settings.json` in the store root (versioned, like core's files; missing or corrupt → defaults); `Settings::load(root: &Path) -> Settings`, `save(&self, root: &Path)`. `pub struct AppMode { pub background: bool }` with `shows_dock_icon(window_open: bool) -> bool`, `bypass_at_start() -> bool`, `quits_when_window_closes() -> bool`, `offers_launch_at_login() -> bool`, ported from the Swift `AppMode`.

- [ ] **Step 1: Write the failing tests** in `desktop/tests/levels_settings.rs`:
  - The 2 Swift `AppModeTests`, as `background_mode_is_a_menu_bar_app` and `foreground_mode_is_an_ordinary_app`.
  - `levels_default_like_the_plugin`: the defaults listed above.
  - `params_scale_strength_and_tone`: with the example profile from core's test support (copy `EXAMPLE_LEFT/RIGHT` into `desktop/tests/support/mod.rs`), `Levels { strength: 50.0, tone: 25.0, .. }.params(Some(&p), true)` equals `fitting::params` with `strength: 0.5, tone: 0.25`.
  - `level_settings_round_trip`: `apply_settings(&l.to_settings())` gives back `l`'s level fields.
  - `settings_persist_and_default_on_corrupt_file` (with a temp dir).
- [ ] **Step 2:** Run `cd desktop && cargo test --test levels_settings`. Expected: compile errors.
- [ ] **Step 3:** Implement `levels.rs` and `settings.rs`.
- [ ] **Step 4:** Run `cd desktop && cargo test`. Expected: all pass.
- [ ] **Step 5:** Commit `feat(app): levels, settings and app mode`.

### Task 4: Profile editor (versions, autosave, errors)

**Files:**
- Create: `desktop/src/model/editor.rs`, `desktop/tests/editor_profiles.rs`

**Interfaces:**
- Consumes: `audiocurve_core::store::{ProfileStore, StoreError, types::{SavedProfile, WorkingCopy}}`.
- Produces (`model::editor`): `pub enum Pane { Audiogram, Gain, Spectrum }`; `pub struct TrimSelection { pub ear: Ear, pub band: usize }`. `pub struct Editor`, with public fields `profile: Option<HearingProfile>`, `source_id: Option<Uuid>`, `saved: Vec<SavedProfile>`, `selection: Option<PointSelection>`, `trim_selection: Option<TrimSelection>`, `active_ear: Ear`, `pane: Pane`, `typed_buffer: String`, `error: Option<String>`, and these methods:
  - `new(store: ProfileStore) -> Editor`: loads the working copy, else `None`.
  - `current_name(&self) -> Option<&str>`
  - `is_dirty(&self) -> bool`
  - `refresh_saved(&mut self)`
  - `load(&mut self, id: Uuid) -> bool`
  - `save(&mut self) -> bool`: overwrites the source, or returns false when there is none.
  - `save_as(&mut self, name: &str)`
  - `rename(&mut self, id: Uuid, name: &str)`
  - `duplicate(&mut self, id: Uuid)`: uses `tr`'d "%@ copy" / "%@ copy %@".
  - `delete(&mut self, id: Uuid)`
  - `add_flat_profile(&mut self)`
  - `set_profile(&mut self, p: HearingProfile, now: Instant)`: marks the working copy for an autosave 0.5 s later.
  - `tick(&mut self, now: Instant)`: writes a due autosave.
  - `next_deadline(&self) -> Option<Instant>`
  - `save_measured(&mut self, id: Uuid, name: &str, p: HearingProfile, m: HearingTestRecord) -> Result<SavedProfile, StoreError>`: unique name, then load it.

  Store errors become `error` texts, as in `PluginViewModel.handle`: "Enter a name", "A version with that name already exists", "Could not save the profile". `error` clears after 4 s through `tick`. All rules are from `AU/PluginViewModel.swift` and the profile part of `AU/AudioCurveAudioUnit.swift`.

- [ ] **Step 1: Write the failing tests** in `desktop/tests/editor_profiles.rs`, each with a temp store:
  - `save_as_then_dirty_after_edit`
  - `save_overwrites_source_and_clears_dirty`
  - `save_without_source_returns_false`
  - `name_taken_sets_error_and_error_clears_after_4s`
  - `duplicate_uses_translated_copy_name`: under `Lang::En` the result is "X copy".
  - `delete_of_loaded_profile_keeps_profile_but_clears_source`: as Swift does.
  - `autosave_is_written_after_half_a_second_and_restored_by_new` (tick at +0.4 s: not written; at +0.5 s: written; `Editor::new` on the same root restores it).
  - `save_measured_picks_a_unique_name_and_loads_it`.
- [ ] **Step 2:** Run `cd desktop && cargo test --test editor_profiles`. Expected: compile errors.
- [ ] **Step 3:** Implement `editor.rs`.
- [ ] **Step 4:** Run `cd desktop && cargo test`. Expected: all pass.
- [ ] **Step 5:** Commit `feat(app): profile editor with versions and autosave`.

### Task 5: Audiogram and trim editing by keyboard and pointer

**Files:**
- Create: `desktop/src/model/editing_input.rs`, `desktop/src/ui/geometry.rs`, `desktop/tests/editing_input.rs`

**Interfaces:**
- Consumes: `Editor` (Task 4), `audiocurve_core::editing`.
- Produces (`ui::geometry`, pure math, no egui types): `pub struct PlotGeometry` and `pub struct GainGeometry`, ported from the Swift `PlotGeometry`/`GainGeometry` in `AU/UI/AudiogramEditorView.swift` and `AU/UI/GainView.swift`. Each has `new(width: f64, height: f64)` (gain also takes `range: RangeInclusive<f64>`), `x(f: f64) -> f64`, `y(v: f64) -> f64` and `hit(x: f64, y: f64) -> Option<…>`. Rects use a local `Rect { min_x, min_y, width, height }`. Zero or negative sizes give a degenerate but finite geometry.
- Produces (`model::editing_input`): `pub enum Key { Up, Down, Left, Right, Tab, Delete, Return, Escape, Char(char) }`; and on `Editor`:
  - `handle_key(&mut self, key: Key, fine: bool, now: Instant) -> bool` (true = handled)
  - `begin_drag(&mut self, x: f64, y: f64, w: f64, h: f64, gain_range: RangeInclusive<f64>, now: Instant)`
  - `drag(&mut self, x: f64, y: f64, w: f64, h: f64, fine: bool, now: Instant)`
  - `end_drag(&mut self)`
  - `reset_trim_at(&mut self, x: f64, y: f64, w: f64, h: f64, gain_range: RangeInclusive<f64>, now: Instant)`
  - `point_label(&self, s: PointSelection) -> String`
  - `trim_label(&self, t: TrimSelection, gain_soft_db: f64) -> String`

  All rules, including typing digits into `typed_buffer` and Enter committing through `parse_typed`, are from `PluginViewModel.handleKey`/`beginDrag`/`drag` and `Fmt`.

- [ ] **Step 1: Write the failing tests** in `desktop/tests/editing_input.rs`:
  - `arrow_up_lowers_threshold_by_5_and_option_by_1`
  - `left_right_move_between_existing_points`
  - `tab_switches_ear_keeping_frequency_or_nearest`
  - `delete_removes_point_and_selects_neighbor`
  - `typing_digits_then_return_sets_value`: "4", "5", Return → 45.
  - `escape_clears_selection_and_buffer`
  - `drag_on_empty_frequency_adds_point_on_active_ear`
  - `trim_drag_is_clamped_to_20`
  - `geometry_round_trips_frequency_and_db`
  - Review Focus 4, `out_of_range_or_garbage_typed_value_is_ignored`: select a point; type "1","3","0", Return, and the profile is unchanged and the buffer empty; the same with "a","b","c".
- [ ] **Step 2:** Run `cd desktop && cargo test --test editing_input`. Expected: compile errors.
- [ ] **Step 3:** Implement `editing_input.rs` and `geometry.rs`.
- [ ] **Step 4:** Run `cd desktop && cargo test`. Expected: all pass.
- [ ] **Step 5:** Commit `feat(app): keyboard and pointer editing of audiogram and trim`.

### Task 6: App model: route, outputs, engine and status

**Files:**
- Create: `desktop/src/model/app.rs`, `desktop/src/model/status.rs`, `desktop/tests/app_model.rs`

**Interfaces:**
- Consumes: `Platform`, `SharedEngine`, `Engine` (Task 1); `Levels`, `Settings`, `AppMode` (Task 3); `Editor` (Task 4); core's `RouteController`, `route_target` types, `ActiveOutputs`, `OutputAssignments`, `output_switch_actions`, `InstanceHeartbeat`, `ProfileStore`.
- Produces (`model::app`): `pub struct Launch { pub from_login_item: bool }`. `pub struct AppModel`, with public fields `settings: Settings`, `levels: Levels`, `editor: Editor`, `bypass: bool`, `window_open: bool`, `current_output: Option<OutputDevice>`, `available_outputs: Vec<OutputDevice>`, `active_outputs: ActiveOutputs`, `assignments: OutputAssignments`, `other_hosts: Vec<String>`, and these methods:
  - `new(platform: Platform, store: ProfileStore, launch: Launch, now: Instant) -> AppModel`
  - `tick(&mut self, now: Instant)`: polls platform events, ticks the route controller, editor, auto level and hearing session, applies params to the engine on change, writes this instance's heartbeat every 2 s and reads other hosts.
  - `next_deadline(&self) -> Option<Instant>`
  - `route_state(&self) -> &RouteTarget`
  - `set_enabled(&mut self, on: bool, now: Instant)`
  - `set_bypass(&mut self, on: bool, now: Instant)`
  - `set_background_processing(&mut self, on: bool)`: applies `AppMode` (Dock icon, Bypass).
  - `set_show_in_menu_bar(&mut self, on: bool)`
  - `set_launch_at_login(&mut self, on: bool)`
  - `set_output_enabled(&mut self, d: &OutputDevice, on: bool, now: Instant)`
  - `assign(&mut self, d: &OutputDevice, profile: Option<Uuid>)`
  - `load_profile(&mut self, id: Uuid, now: Instant)`
  - `retry_engine(&mut self, now: Instant)`
  - `quit(&mut self)`: route torn down first.
  - `engine(&self) -> Arc<Engine>` (for meters and spectrum)
  - `processing_active(&self) -> bool`

  An output switch runs `output_switch_actions` and applies `Overwrite`, `SaveUnsaved`, `ApplyLevels` and `Load` in order, as `AudioCurveAudioUnit.outputDeviceChanged` does. Level changes on an assigned output are saved to its assignment 1 s after the last change. `CaptureLooksDenied` sets the permission to `Denied`; turning AudioCurve on again resets it to `Unknown`.
- Produces (`model::status`): `pub enum Tone { Positive, Warning, Neutral }`; `pub fn status_text(m: &AppModel) -> (String, Tone)`. It carries the texts of `HostView.StatusLine`, except that `Failed(Plugin)` becomes `tr("The sound processing stopped")`, and a running hearing test shows "Hearing test in progress".

- [ ] **Step 1: Write the failing tests** in `desktop/tests/app_model.rs`, with the fake platform and a temp store. Call a helper `settle(&mut m, &mut t)` that ticks at `next_deadline()` until there is none or 5 s have passed.
  - `starts_running_on_the_current_output` (`route_log == ["build:airpods"]`, status "Active – AirPods Pro" with `Tone::Positive`)
  - `foreground_mode_starts_bypassed`
  - `turning_off_tears_down`
  - `unticked_output_pauses_and_says_so`
  - `output_change_loads_the_assigned_profile_and_levels`
  - Review Focus 1, `output_switch_saves_unsaved_edits_first`: edit the loaded profile (dirty); switch to an output assigned to another profile; then the old source on disk has the edits and the new profile is loaded.
  - `levels_on_an_assigned_output_are_saved_after_1s`
  - `render_failure_rebuilds_engine_and_route`: `route_log` ends with `teardown:airpods`, `build:airpods`; `Arc::ptr_eq` shows the engine changed.
  - Review Focus 3, `second_engine_failure_within_10s_latches_and_try_again_recovers`: status "The sound processing stopped" with `Tone::Warning`; `retry_engine` → running again.
  - `capture_denied_shows_permission_status_until_turned_off_and_on`
  - `other_live_hosts_are_reported`: write a foreign heartbeat into the store.
  - `quit_tears_down_first`
- [ ] **Step 2:** Run `cd desktop && cargo test --test app_model`. Expected: compile errors.
- [ ] **Step 3:** Implement `app.rs` and `status.rs`.
- [ ] **Step 4:** Run `cd desktop && cargo test`. Expected: all pass.
- [ ] **Step 5:** Commit `feat(app): app model with route, outputs, engine and status`.

### Task 7: Auto level and pending-volume restore

**Files:**
- Create: `desktop/src/model/auto_level.rs`, `desktop/tests/auto_level_restore.rs`
- Modify: `desktop/src/model/app.rs`

**Interfaces:**
- Consumes: `AppModel` (Task 6), core's `AutoLevelMeasurement`, `ProfileStore` pending volumes, `Volume` trait.
- Produces: `pub enum AutoLevelUi { Idle, Measuring { remaining: u32 }, Done(f64), TooQuiet }`. On `AppModel`: `start_auto_level(&mut self, now: Instant)`, which samples meters every 50 ms for 60 blocks, then sets `levels.output_gain_db` to the result rounded to 0.1 dB; and `auto_level(&self) -> &AutoLevelUi`. Also `restore_pending_volumes(&mut self, except: Option<&str>)`, called at start and on `OutputsChanged`, which follows `AppModel.restorePendingVolumes` in Swift.

- [ ] **Step 1: Write the failing tests:**
  - `auto_level_measures_60_blocks_then_sets_output_gain`: pump noise between ticks; the state goes `Measuring{60}` … `Done(g)`, and `levels.output_gain_db == g`.
  - `auto_level_with_silence_is_too_quiet`
  - `pending_volume_is_restored_at_start_and_cleared`: store a pending snapshot first; `volume_log` has `restore:airpods` and the store has none left.
  - `pending_volume_of_absent_device_waits_until_it_returns`
- [ ] **Step 2:** Run `cd desktop && cargo test --test auto_level_restore`. Expected: compile errors.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Run `cd desktop && cargo test`. Expected: all pass.
- [ ] **Step 5:** Commit `feat(app): auto level and pending volume restore`.

### Task 8: Hearing test session

**Files:**
- Create: `desktop/src/model/hearing.rs`, `desktop/tests/hearing_session.rs`
- Modify: `desktop/src/model/app.rs` (`open_hearing_test`, `close_hearing_test`, `hearing(&mut self) -> Option<&mut HearingSession>`, and the route's `testing` input while a session is active)

**Interfaces:**
- Consumes: core's `HearingTest`, `TestCalibration`, `Transducer`, `HearingTestRecord`, `TestWarning`; the `TestTone`, `Volume` and `Activity` traits; `Editor::save_measured`.
- Produces (`model::hearing`):
  - `pub enum Phase { Intro, Running(Presentation), Paused(PauseReason), Done(HearingTestResult), SaveFailed }`
  - `pub enum PauseReason { User, OtherSound }`
  - `pub enum StopReason { OutputChanged, OtherHost, WindowClosed, GeneratorStopped, GeneratorFailed, VolumeNotSet, LowSampleRate }`
  - `pub struct HearingSession`, with public read-only state `phase`, `stop_reason`, `transducer_choice: Option<Transducer>`, `others_playing`, `muted`, `volume_settable`, `low_sample_rate`, `remind_volume_down`, and these methods:
    - `can_start(&self) -> bool`
    - `can_resume(&self) -> bool`
    - `transducer(&self) -> Transducer`
    - `default_name(&self, formatted_date: &str) -> String`

  `AppModel` drives the session through `hearing_start`, `hearing_respond`, `hearing_pause`, `hearing_resume`, `hearing_cancel`, `hearing_discard`, `hearing_save(name: &str, assign: bool)`. The session runs on `tick(now)` with the Swift timings: gap from the presentation, tone 1.0 s, response window 1.5 s, environment check every 1 s. Low sample rate means below 44 100 Hz. Order and safety rules are those in the header of `App/HearingTestModel.swift`:
  - The route is paused before the tone generator starts.
  - The snapshot is saved before the volume goes to max.
  - On every pause, stop or end, the tones stop first, then the volume is restored and the pending snapshot cleared.
  - A failed restore keeps the snapshot.

  Saving is synchronous here: there is no plugin round trip. It calls `Editor::save_measured`, assigns the profile when asked, and closes the session. On an error the phase becomes `SaveFailed`.

- [ ] **Step 1: Write the failing tests** in `desktop/tests/hearing_session.rs`. Use a fake listener that calls `hearing_respond` when the fake tone log shows a tone at or above 35 dB HL, and drive the clock through `next_deadline()`.
  - `full_run_saves_a_profile_assigned_to_the_output`
  - `start_is_refused_when_muted_or_others_playing_or_other_host`
  - `volume_is_saved_before_max_and_restored_after_finish`: `volume_log == ["max:airpods", …, "restore:airpods"]`, and the pending snapshot in the store was written before `max`.
  - `route_is_paused_during_the_test_and_rebuilt_after`
  - `others_playing_pauses_and_resume_rechecks`
  - Review Focus 2, `output_change_stops_the_test_and_restores_volume`: `stop_reason == Some(OutputChanged)`, `tone_log` ends with `stop`, `volume_log` ends with `restore:airpods`.
  - `cancel_restores_volume_and_closes`
  - `failed_restore_keeps_the_pending_snapshot`
- [ ] **Step 2:** Run `cd desktop && cargo test --test hearing_session`. Expected: compile errors.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Run `cd desktop && cargo test`. Expected: all pass.
- [ ] **Step 5:** Commit `feat(app): hearing test session`.

### Task 9: Menu bar menu model

**Files:**
- Create: `desktop/src/model/menu.rs`, `desktop/tests/menu_model.rs`
- Modify: `desktop/src/platform/mod.rs` (add `pub trait Tray { fn set_menu(&mut self, items: &[MenuItem]); fn set_visible(&mut self, visible: bool); fn poll_actions(&mut self) -> Vec<MenuAction>; }` and `tray: Box<dyn Tray>` on `Platform`), `desktop/src/platform/fake.rs` (`FakeControl::menu() -> Vec<MenuItem>`, `click(MenuAction)`)

**Interfaces:**
- Produces (`model::menu`):
  - `pub enum MenuItem { Check { title: String, checked: bool, enabled: bool, action: MenuAction }, Text(String), Header(String), Submenu { title: String, items: Vec<MenuItem> }, Separator, Action { title: String, action: MenuAction } }`
  - `pub enum MenuAction { ToggleEnabled, OpenCaptureSettings, LoadProfile(Uuid), ToggleOutput(String), AssignProfile { uid: String, profile: Option<Uuid> }, OpenWindow, OpenSettings, Quit }`
  - `pub fn build_menu(m: &AppModel) -> Vec<MenuItem>`: the layout of the spec's menu sketch. The status line is the `status_text`. "Open System Settings" appears only for `Failed(Permission)`, and the other-host warning only when `other_hosts` is not empty. The last checked output is disabled. "Profile for <name>" submenus list "—" plus the saved profiles.
  - `AppModel::handle_menu(&mut self, a: MenuAction, now: Instant)`
  - `AppModel::tick` pushes `build_menu` to the tray when it changed, and polls its actions.

- [ ] **Step 1: Write the failing tests** in `desktop/tests/menu_model.rs`:
  - `menu_has_the_sketched_layout`: compare titles and kinds for the demo outputs.
  - `last_checked_output_is_disabled`
  - `permission_item_only_when_denied`
  - `profile_submenu_checks_the_assigned_profile`
  - `clicking_items_changes_the_model` (toggle output, assign, load, quit)
- [ ] **Step 2:** Run `cd desktop && cargo test --test menu_model`. Expected: compile errors.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Run `cd desktop && cargo test`. Expected: all pass.
- [ ] **Step 5:** Commit `feat(app): menu bar menu model`.

### Task 10: Theme, widgets and plots

**Files:**
- Create: `desktop/src/ui/mod.rs`, `desktop/src/ui/theme.rs`, `desktop/src/ui/widgets.rs`, `desktop/src/ui/audiogram.rs`, `desktop/src/ui/gain.rs`, `desktop/src/ui/spectrum.rs`, `desktop/tests/ui_plots.rs`

**Interfaces:**
- Consumes: `ui::geometry` (Task 5), `Editor` and its input methods (Tasks 4–5), `AppModel::engine()` (Task 6).
- Produces:
  - `ui::theme::apply(ctx: &egui::Context)`: the Global Constraints theme, for both light and dark styles.
  - `ui::theme::{LEFT, RIGHT, NORMAL_BAND}: egui::Color32`.
  - `ui::widgets::segmented<T: PartialEq + Copy>(ui, value: &mut T, items: &[(T, &str)]) -> egui::Response`
  - `ui::widgets::labeled_slider(ui, label: &str, value: &mut f64, range: RangeInclusive<f64>, value_text: &str) -> egui::Response`
  - `ui::widgets::meter_pair(ui, label: &str, left_db: f32, right_db: f32, peak_db: Option<[f32; 2]>)`
  - `ui::audiogram::show(ui, editor: &mut Editor, now: Instant)`, `ui::gain::show(ui, m: &mut AppModel, now: Instant)`, `ui::spectrum::show(ui, m: &AppModel, ears: &mut SpectrumEars)`, with `pub enum SpectrumEars { Left, Right, Both }`. Each draws what the matching Swift view draws (`AudiogramEditorView`, `GainView`, `SpectrumView`) with the egui painter, and turns pointer and key events into the Task 5 calls.

- [ ] **Step 1: Write the failing tests** in `desktop/tests/ui_plots.rs` with `egui_kittest::Harness`, the default accessibility check on, the fake platform and the example profile:
  - `audiogram_renders_light_and_dark` (`harness.render()` succeeds for both themes)
  - `clicking_a_point_selects_it`: click at the screen position of the left 1 kHz point from `PlotGeometry`; `editor.selection == Some(Left, 1000)`.
  - `dragging_a_point_changes_its_threshold`
  - `gain_and_spectrum_render_after_pumping_noise`
  - `segmented_control_selects_on_click`
  - Review Focus 5, `tiny_and_zero_sized_plots_do_not_panic`: render each plot at 600×420 and inside a 0-height `allocate_ui`; and `PlotGeometry::new(0.0, 0.0).y(50.0).is_finite()`.
- [ ] **Step 2:** Run `cd desktop && cargo test --test ui_plots`. Expected: compile errors.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Run `cd desktop && cargo test`. Expected: all pass.
- [ ] **Step 5:** Commit `feat(app): tuned theme, widgets and plots`.

### Task 11: Main window

**Files:**
- Create: `desktop/src/ui/main_window.rs`, `desktop/src/ui/controls.rs`, `desktop/src/ui/profile_menu.rs`, `desktop/tests/ui_main_window.rs`

**Interfaces:**
- Consumes: everything above.
- Produces: `ui::main_window::show(ctx: &egui::Context, m: &mut AppModel, now: Instant)`, which draws the hearing test (Task 12) instead of the plugin view while a session is open. Layout and texts follow `App/HostView.swift` and `AU/UI/PluginView.swift`:
  - Bypass (A/B) checkbox and the status line
  - pane segmented control, profile menu and the "Hearing threshold, dB HL" caption
  - the plot
  - Left/Right segmented control
  - the controls panel, from `AU/UI/ControlsPanel.swift`: Speech/Music/Auto, "Auto: speech/music", Strength, Tone with Clear/Warm, Input level, Output level, Ceiling (MPO), Compression, Limiter (turning it off asks "Turn Off" / "Cancel" in a modal), the "Active on" popup with checkboxes and the "Use on <output>" item, Auto level, and the In/Out/Safety meters
  - the profile menu, from `AU/UI/ProfileMenu.swift`: the versions, Save, Save As…, Rename…, Duplicate, Delete… with confirmation, and name prompts in an `egui::Modal`
  - the "Hearing test…" button

- [ ] **Step 1: Write the failing tests** in `desktop/tests/ui_main_window.rs` (Harness 900×720):
  - `main_window_renders_light_and_dark`
  - `limiter_off_asks_for_confirmation`: click "Limiter"; a modal with "Turn Off" appears; click it; `levels.limiter == false`.
  - `save_as_prompt_saves_a_version`: type a name, then the version is in `editor.saved`.
  - `bypass_checkbox_sets_bypass`
  - `status_line_shows_the_status_text`: query by label.
  - `swedish_ui_renders`: `set_lang(Sv)`; the "Active on" label shows its Swedish text.
- [ ] **Step 2:** Run `cd desktop && cargo test --test ui_main_window`. Expected: compile errors.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Run `cd desktop && cargo test`. Expected: all pass.
- [ ] **Step 5:** Commit `feat(app): main window`.

### Task 12: Hearing test screens and Settings window

**Files:**
- Create: `desktop/src/ui/hearing_test.rs`, `desktop/src/ui/settings.rs`, `desktop/tests/ui_hearing_settings.rs`

**Interfaces:**
- Produces: `ui::hearing_test::show(ui, m: &mut AppModel, now: Instant)`, with the intro, running, paused, done/save and save-failed screens, layout and texts from `App/HearingTestView.swift`. Warnings use `TestWarning::from_flags`. Dates are formatted with `formatted_date(SystemTime, Lang) -> String` in the local time zone, with a small per-language table that reproduces macOS's abbreviated style (the expected strings are in the test below). Also `ui::settings::show(ctx, m: &mut AppModel, open: &mut bool)`: a separate viewport window with the three toggles from `App/SettingsView.swift`; "Launch at login" is shown only when `AppMode::offers_launch_at_login`.

- [ ] **Step 1: Write the failing tests:**
  - `each_hearing_phase_renders`: build a session in each phase through `AppModel` and the fake platform, and render it.
  - `i_hear_it_button_responds`
  - `done_screen_saves_with_default_name`: "AirPods Pro – 16 Nov 2026" under `Lang::En` for a fixed date.
  - `formatted_date_in_every_language`: for 2026-11-16, exactly what macOS's `Date.FormatStyle(date: .abbreviated)` gives: En "16 Nov 2026", Sv "16 nov. 2026", Da "16. nov. 2026", Nb "16. nov. 2026", Fi "16.11.2026", De "16. Nov. 2026", Fr "16 nov. 2026", Es "16 nov 2026".
  - `settings_hides_launch_at_login_in_foreground_mode`
- [ ] **Step 2:** Run `cd desktop && cargo test --test ui_hearing_settings`. Expected: compile errors.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Run `cd desktop && cargo test`. Expected: all pass.
- [ ] **Step 5:** Commit `feat(app): hearing test screens and settings`.

### Task 13: Binary and screenshots

**Files:**
- Create: `desktop/examples/screenshots.rs`, `desktop/README.md`
- Modify: `desktop/src/main.rs`, `Makefile` (target `egui-screenshots`)

**Interfaces:**
- Produces:
  - `desktop/src/main.rs`: an `eframe` app titled "AudioCurve" with a minimum inner size of 600×420 and a default of 900×720. It runs `AppModel` on the fake platform with `ProfileStore::standard()` and `Lang::from_locale(sys_locale::get_locale())`, calls `theme::apply`, calls `m.tick(Instant::now())` every frame, and asks for a repaint at `next_deadline()` and at least every 50 ms while meters are shown.
  - `cargo run --example screenshots -- <out dir>`: with a temp store and the example profile, it renders audiogram, gain, spectrum, hearing-test-running and hearing-test-result screens, light and dark, to `<name>-light.png` and `<name>-dark.png` with `egui_kittest` at 2× scale.
  - `make egui-screenshots`: runs the example into `build/egui-screenshots`.

- [ ] **Step 1: Write the failing test** `desktop/tests/screenshots.rs::screenshot_example_writes_ten_images`: run the example's `render_all(dir)` function (expose it from `desktop/src/screenshots.rs`, which the example calls) into a temp dir and assert the ten files exist with width 1800.
- [ ] **Step 2:** Run `cd desktop && cargo test --test screenshots`. Expected: compile errors.
- [ ] **Step 3:** Implement `src/screenshots.rs`, the example, `main.rs`, the Makefile target and `desktop/README.md`, which says what stage 1 is and how to run it, test it and make screenshots.
- [ ] **Step 4:** Run `cd desktop && cargo test`, `make egui-screenshots`, and `cargo run` (opens the window: check by hand that it runs, then quit). Expected: tests pass, ten PNGs in `build/egui-screenshots`, and the window shows the demo outputs with moving meters.
- [ ] **Step 5:** Commit `feat(app): runnable fake-platform app and screenshots`.
