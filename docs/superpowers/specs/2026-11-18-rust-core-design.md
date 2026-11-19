# Rust core: porting AudioCurveCore to Rust (port step 1)

Date: 2026-11-18
Status: approved by the user (2026-11-18)

## Intent

AudioCurve is to run on Linux and Windows as well as macOS. The user wants as little Swift as possible and as much as possible in Rust. The UI will be egui (with a tuned theme), and the plugin will be built with nih-plug. The DSP kernel (`dsp/`) is already Rust.

This step moves the platform-independent logic in `Packages/AudioCurveCore` (Swift, ~1,900 lines without tests) into a new Rust crate. Later steps build on it:

1. **This step:** Rust core.
2. nih-plug plugin (CLAP/VST3, AUv2 via clap-wrapper).
3. egui app with an audio backend trait (CoreAudio first, then PipeWire, then WASAPI).
4. Linux, then Windows.

## Goals

- A Rust crate, `audiocurve-core`, that does everything the platform-independent part of `AudioCurveCore` does: prescription, hearing test, calibration, warnings, tone synthesis, auto level, audiogram editing, profiles and their storage, per-output assignments, active outputs, route state machine, volume snapshots, instance heartbeats, plugin state.
- Same behaviour as the Swift code, shown by porting the Swift tests (186 `@Test`s) to Rust and reading the shared fixture files from where they are.
- No platform-specific code, so that a later port to Linux and Windows is easier. This step does not build or test it there.

## Non-goals

- **The Swift app does not use the Rust core.** No FFI, no changes to Swift code or the Xcode/Makefile build of the app. The Swift app keeps working as today until the egui app replaces it (approach B, chosen by the user).
- **No compatibility with existing saved files.** The user decided that the Rust core's file format is free: users create new audiograms. The Rust store does not read the Swift JSON files.
- No UI, no audio I/O, no plugin wrapper (later steps).
- **No port to Linux or Windows** (decided by the user): no Linux/Windows CI, builds or fixes in this step. It only prepares the code so the port is easier later.
- No change to the DSP kernel's behaviour.

## Anti-goals

- Code written only to be thrown away when the Swift app goes (FFI glue, Swift wrappers).
- Macro-level rewrites of the logic "while we're at it". Behaviour changes belong in their own change, after the port, so the Swift tests stay a valid oracle.
- Any dependency that pulls platform UI or audio code into the core.

## Constraints

- **The Swift build must keep working unchanged.** `project.yml` and the `Makefile` link `dsp/target/aarch64-apple-darwin/release/libaudiocurve_dsp.a`. A Cargo workspace at the repository root would move `dsp`'s build output to `target/` at the root and break that. So `core/` is a **standalone crate** with a path dependency on `../dsp`, not a workspace (see Calls made).
- `dsp` stays realtime-safe and dependency-free: `core` depends on `dsp`, never the other way round.
- Edition 2024, like `dsp`.
- Dependencies kept small: `serde`, `serde_json`, `uuid` (with `v4` and `serde`). Anything else needs a reason.

## Approaches

Two questions were settled with the user.

**Should the Swift app start using the Rust core in this step?**

- A. Yes, via FFI. This proves the core in real use at once, but needs ~500 lines of C header and Swift glue for stateful parts (hearing test, profile store, route controller), all thrown away when the egui app replaces the Swift app.
- **B. No: the Rust core stands alone, verified against the Swift tests and fixtures.** It catches the same mistakes as A without throwaway code. **Chosen.**

**Where does the code live?**

- Put the logic in `dsp`: rejected, because `dsp` is the realtime kernel and must not take on serde, files and allocation-heavy logic.
- Several small crates (prescription, hearing test, store …): more bookkeeping than the ~1,500 lines justify.
- **One new crate `core/` next to `dsp/`, depending on it.** **Chosen.**

## Scope: what is ported

| Swift (Sources/AudioCurveCore) | Rust module | Notes |
|---|---|---|
| `Bands`, `Audiogram`, `HearingProfile` | `profile` | Validation errors as an enum. Band centres come from `audiocurve_dsp::CENTERS`. |
| `FittingRule` | `fitting` | `bands`, `blend`, `gain_db`, `interpolated_hl`. `params()` builds `audiocurve_dsp::Params` directly (the Rust struct, not `ac_params`). |
| `TestCalibration`, `HearingTest`, `HearingTestRecord`, `TestWarning` | `hearing_test` | Random source injected (`FnMut() -> f64`), as in Swift. |
| `ToneSynth` | `tone` | Sample-by-sample, same envelope and cap. |
| `AutoLevel` | `auto_level` | Takes `audiocurve_dsp::Meters`. |
| `AudiogramEditing` | `editing` | Including `parse_typed` and `db_hl_at_y`. |
| `ActiveOutputs`, `OutputAssignments`, `OutputSwitch` | `outputs` | |
| `ProfileStore` | `store` | New file format (below). |
| `RouteDecision`, `RouteController` | `route` | Platform-independent state machine (below). |
| `VolumeSnapshot` | `volume` | Pending snapshots move from UserDefaults to a JSON file in the store root. |
| `InstanceHeartbeat` | `heartbeat` | |
| `PluginState` | `plugin_state` | The plugin in step 2 persists its profile through it. |

**Not ported:**

- `OutputDeviceMonitor` (CoreAudio): replaced by the audio backend in step 3.
- `PluginChannel` (talk between the app and the AUv3 via `fullState`): the egui app will run the kernel in-process, so there is no channel.
- `AppMode` (Dock icon and quit-on-close policy): UI policy, belongs to the egui app.
- The layout tests for `ac_params` / `ac_meters`: they guard the C ABI, which `core` does not use. `dsp`'s own `tests/layout.rs` still guards it.

The tests for `PluginChannel`, `AppMode` and the layout tests are not ported either. Nor are the tests that only guard compatibility with old Swift data, since the new format does not read it: `oldSingleSlotValueIsStillRead`, `snapshotWithoutAttemptsStillDecodes`, `pendingVolumeUsesTheDocumentedKey` and the "old files decode" half of `measurementRoundTripsAndOldFilesDecode` (its round-trip half is ported). Every other Swift test gets a Rust counterpart.

**Plugin state is free too.** The new plugin (CLAP/VST3, AUv2 via clap-wrapper) is a different plugin from today's AUv3, so DAW projects saved with the AUv3 will not load it anyway. `PluginState` uses the same new serde shape as the rest.

## Design notes

**Data types.** Plain Rust structs and enums mirroring the Swift ones, with `serde` derives where they are stored. Swift `Double` → `f64`, `UUID` → `uuid::Uuid`, `Date` → `std::time::SystemTime`, stored as Unix seconds (`f64`).

**Storage format.** JSON, one file per thing, the same layout as today under a root directory: `profiles/<uuid>.json`, `current.json`, `outputs.json`, `assignments.json`, `instances/<pid>.json`, plus `pending-volume.json` (new, replaces the UserDefaults key). Each file is `{"version": 1, "value": …}` like today; readers drop files with another version or that fail to parse or validate, as today. Writes are atomic: write a temporary file in the same directory, then rename. The caller passes the root; a helper returns the platform default (macOS `~/Library/Application Support/audiocurve`, Linux `$XDG_DATA_HOME/audiocurve`, Windows `%APPDATA%\audiocurve`).

**Store behaviour** is the Swift behaviour: names trimmed and truncated to 60 characters, case-insensitive uniqueness, `" (n)"` suffixes for unique names, `duplicate` with a caller-supplied copy-name function (the UI localizes it), sorting by name. Swift sorts with `localizedStandardCompare`; Rust sorts by case-folded name with digit runs compared as numbers (a "natural" sort), which matches for the names the tests use.

**Route controller.** The Swift `RouteController` is `@MainActor` and schedules work with an injected `after(delay, closure)`. Closures that capture the controller do not fit Rust well, so the Rust version is driven from outside:

- It owns a `Box<dyn RouteBuilder>`; `build(device_uid) -> Result<Box<dyn BuiltRoute>, Error>`, and `BuiltRoute::tear_down(self)` does the platform's teardown in the right order (on macOS: stop IO, destroy aggregate, destroy tap).
- Instead of scheduling, it records a deadline. The owner calls `tick(now)` when `next_deadline()` is due (or on any event) and the controller does what the Swift timer callback would have done. All times come in as arguments, so the tests need no clock.
- Like the Swift `@MainActor` version, it is driven from one thread (the UI thread). It and the trait objects it owns are not required to be `Send`; on macOS the built route holds CoreAudio handles.
- State changes are returned from the calls (or read from `state()`), not pushed through a callback; `pluginDied` / `retryPlugin` stay, and the "reload the plugin" request is returned as a flag.

The rules (coalescing 0.2 s, retry after 0.5 s, second death within 10 s latches `failed`, every path out of `running` tears down first, `shut_down` ignores later inputs) are unchanged, and the 16 Swift tests are ported against the new driving style.

**Hearing test.** A direct port. The Swift tests drive it with scripted random values; the Rust tests use the same values, so the same presentations come out.

**Default profile name.** Swift's `TestWarning.defaultName` formats a date with `Date.FormatStyle` and a locale. Date formatting is the UI's job: the Rust function takes the date already formatted as a string and only does the composition and truncation.

## Verification

- Every ported Swift test has a Rust test with the same name in snake case, so the two can be compared side by side.
- `prescription-cases.json` and any other fixture are read from `Packages/AudioCurveCore/Tests/AudioCurveCoreTests/Fixtures/` with `include_str!`, not copied.
- `cargo test` in `core/` passes on macOS. CI (`.github/workflows/release.yml`) and `make test` also run `core`'s tests next to `dsp`'s. Building and testing on Linux and Windows is not part of this step (see Non-goals).
- `cargo clippy -- -D warnings` is clean.
- The Swift tests and the app build are untouched and still pass.

## Calls made (not discussed with the user)

- **Standalone crate instead of a workspace**, to keep `dsp/target/…` where the Swift build expects it. A workspace can come when the Swift build is retired.
- **The prescription copy in `dsp/tests/support/measure/prescription.rs` stays** for now. Having `dsp`'s tests use `core` would mean a dev-dependency cycle that gives two copies of `dsp`'s types. Both copies are checked against the same fixture. They can be merged when the measurement tool moves to `core` or a later step.
- `RouteController` keeps the plugin-death logic as it is. The egui app's engine runs in-process, but the backend can still fail and the latch is still useful; whether to simplify it is decided in step 3.
- Pending volume snapshots move from UserDefaults to a JSON file.
- Natural sort instead of `localizedStandardCompare`.
- Swift tests that only guard old data formats are not ported, following the user's decision that old files need not be read; plugin state is likewise free.
- The route controller is single-threaded with no `Send` bounds.

## Left to the builder

- Module boundaries inside the crate and the exact Rust API names, as long as the mapping to the Swift tests stays readable.
- Error types (`thiserror` is fine if wanted, or hand-written enums).
- Whether `ToneSynth` renders per sample or per buffer.
