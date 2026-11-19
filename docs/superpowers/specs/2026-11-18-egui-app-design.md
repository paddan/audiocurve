# egui app: AudioCurve as a Rust app on macOS (after the Rust core, before the nih-plug plugin)

Date: 2026-11-18
Status: approved by the user (2026-11-18)

## Intent

AudioCurve is to be ported to Linux and Windows later. The user wants the code as ready for that port as possible, with as little Swift as possible. The port itself is **not** part of this work: everything here runs on macOS only.

This design follows the Rust core ([2026-11-18-rust-core-design.md](2026-11-18-rust-core-design.md), approved), which moves the logic of `AudioCurveCore` into the `audiocurve-core` crate. This step builds on it: a Rust app with an egui UI that does everything today's Swift app (`App/`) does and replaces it. Once it is done, porting the app means writing a Linux and a Windows audio backend and the few platform services listed below.

## Goals

- A Rust app, `AudioCurve.app`, that replaces today's Swift app with the same features:
  - **Main window:** Bypass (A/B), status line, Audiogram / Gain / Spectrum views, the audiogram editor (drag, type, keyboard, per-band trim), the profile menu (save, save as, rename, duplicate, delete), the controls panel (Speech / Music / Auto, Strength, Tone, Input level, Output level, Ceiling, Compression, Limiter, Active on, Auto level), the In/Out/Safety meters, the limiter-off confirmation.
  - **Menu bar** (the ear): on/off, status, profile, Active on with a profile per output, Open AudioCurve…, Settings…, Quit.
  - **Settings:** process system audio in the background, show in menu bar, launch at login.
  - **Hearing test:** the whole flow as today, including In-ear/Over-ear, volume to max and restore (also after a crash or disconnect), the checks that refuse to start (muted output, other audio playing), the warnings, saving the result as a profile assigned to the output.
  - **Profile per output:** switching output loads that output's profile and level settings.
  - **System-wide processing:** the same CoreAudio process tap route as today, with the DSP kernel running in-process.
  - **Languages:** the same eight as today (en, sv, da, nb, fi, de, fr, es), with today's translations.
  - **Screenshots:** `make screenshots` draws the window and the hearing test without a screen, as today. The menu is now a native menu that cannot be drawn without a screen, so the README's two panel images (`panel-light.png`, `panel-dark.png`) are removed rather than mocked.
- The UI and app logic contain no macOS code. Everything macOS-specific sits behind traits, so that Linux and Windows need only new implementations.
- The UI uses the tuned egui theme the user picked (SF font on macOS, Apple-blue accent, 7 px widget rounding, slider fill, 12 px panel rounding).

## Non-goals

- **No port to Linux or Windows** (decided by the user). No builds, CI or backends for them.
- **The AUv3 plugin stays in Swift**, embedded in `AudioCurve.app` as today, until the nih-plug step. So does `Packages/AudioCurveCore`, which the plugin uses.
- No new features and no changes to how things behave, except where this document says so.

## Anti-goals

- macOS code in the UI or app logic crates.
- An app that works but looks clearly worse than today's in everyday use. The tuned theme is the floor, not stock egui.
- Losing the safety guarantees: the Mac is never left muted (every path out of running tears down the route) and the hearing test's volume is always restored.

## Approaches

Three questions were settled with the user.

**What does the egui UI connect to?**

- **A. A complete Rust app that replaces the Swift app**: egui UI, Rust core and DSP kernel in one process, with the audio in Rust behind a backend trait. It is the most work, with `unsafe` CoreAudio code, but at the end almost everything the port needs is done and tested for real. **Chosen**, built in two stages (below).
- B. An egui UI library with a fake engine only, the Swift app keeping the audio. Less work, but the meeting between UI and real audio is first tested during the port.
- Embedding egui in the Swift app: rejected, it needs FFI glue that is thrown away.

**What is the menu bar?**

- **A native menu (NSMenu)**, as other menu bar apps have: native look, and the same code works on Windows and Linux. The on/off switch becomes a check item, a profile per output becomes submenus, and the "running in another app" warning becomes plain text. **Chosen.**
- An egui window under the ear that looks like today's panel: needs own code for placement and for closing when you click outside, and is worse on Linux.

**Profiles shared between the app and the plugin in other hosts.** Today the AUv3 owns the profiles (in its sandbox container) and the app reads them through it, so a profile made in the app shows up in the plugin in, for example, Logic. The Rust app keeps its own store in the core's new format, so that link breaks:

- **Accept the gap**: the app and the AUv3 in other hosts each have their own profile list until the nih-plug step, when the plugin moves to the Rust core and the same storage. Nothing is lost; the plugin keeps its profiles. **Chosen.**
- Do the nih-plug step first: no gap, but the replacement comes later.
- Let the Rust app read and write the AUv3's Swift files: against the new-format decision and needs throwaway glue.

The same gap affects the "AudioCurve is also running in <app>" warning: the Rust app does not see the AUv3's heartbeats, so the warning only covers other instances of the Rust app until the nih-plug step.

## Stages

**Stage 1: UI with a fake engine.** The crates `desktop/` (UI and app logic) and the `Engine`/platform traits, with a fake implementation: made-up outputs, a fake route that "runs", meters and spectrum from the real DSP kernel fed with generated noise (as `make screenshots` does today), a fake hearing-test listener. It runs as an ordinary window on macOS and renders screenshots. The Swift app is untouched and still ships.

**Stage 2: macOS backend.** The CoreAudio implementation of the traits, the menu bar, Settings, launch at login, the Dock icon policy, packaging. At the end of this stage, `make release` and the release workflow build the Rust app (with the Swift AUv3 embedded) and the Swift `App/` target is deleted.

## Structure

Three standalone crates next to each other, as the core design explains (a root workspace would move `dsp/target`, which the Swift AUv3 build still links):

- `dsp/`: unchanged.
- `core/`: the Rust core.
- `desktop/`: the binary, depending on `core` and `dsp`. Inside it:
  - `ui/`: egui views and the theme. No platform code.
  - `model/`: app state and logic, today's `AppModel`, `PluginViewModel` and `HearingTestModel` (the parts that are not already in `core`). No platform code.
  - `platform/`: the traits below, with `fake` and `macos` implementations (`macos` behind `#[cfg(target_os = "macos")]`).

**Platform traits** (names are the builder's; the boundaries are what matters):

- **Audio**: list outputs and the current output, notify on changes (today's `OutputDeviceMonitor`); build and tear down the system-wide route for an output, running the DSP kernel in the IO callback (core's `RouteBuilder`); report a dead render; report "input silent while other apps play" so the model can infer a denied capture permission, as today.
- **Volume**: read, set and restore an output's volume scalars, and tell whether it is muted (for the hearing test and `VolumeSnapshot`).
- **Test tone output**: play a tone buffer from core's `ToneSynth` to an output, bypassing the route (which is paused during the test, as today).
- **Activity**: whether another app is playing audio (the hearing test refuses to start).
- **System**: launch at login get/set; Dock icon shown or hidden; open the system's audio-capture privacy settings; display name of another app.
- **Tray**: the menu bar item and its menu.

The DSP kernel is the Rust `audiocurve_dsp::kernel::Handle`, shared between the model (parameters, meters, spectrum) and the audio callback (processing). It is already `Send + Sync` with lock-free parameter updates.

## UI notes

- **Window**: one main window, as today's `PluginView` hosted by `HostView`. The hearing test replaces the window's content while it runs, as today.
- **Menu bar**: native menu, sketched:
  ```
  ✓ AudioCurve on
  Active – AirPods Pro                (disabled text line)
  ─────────────
  Profile                    ▸ Example ✓ / Example (quiet)
  ─────────────
  Active on                           (disabled header)
  ✓ AirPods Pro (current)             (check item: toggles the output)
    Hearing Aids (not connected)
  ✓ Mac Studio Speakers
  ─────────────
  Profile for AirPods Pro    ▸ — / Example ✓ / …
  Profile for Hearing Aids   ▸ — ✓ / Example / …
  Profile for Mac Studio Speakers ▸ …
  ─────────────
  Open AudioCurve…
  Settings…
  Quit AudioCurve
  ```
  In NSMenu an item with a submenu cannot also be a check toggle, so the outputs appear twice: once as check items under "Active on" (the last checked one disabled, as today), once as "Profile for <output>" submenus. Both labels already exist in the translations. "Open System Settings" (permission denied) and the other-host warning appear as items when they apply, as in today's panel.
- **Settings**: its own small egui window.
- **Theme**: the tuned theme from the comparison. Its values, as rendered and approved by the user: proportional font SF (`/System/Library/Fonts/SFNS.ttf` on macOS, egui's default elsewhere), Body and Button text 14 pt; button padding 10×4, item spacing 8×6, interact height 24; accent `#0A6EF5` for selection fill (white selection stroke) and links; `slider_trailing_fill` on; rounding 7 on all widget states, 12 on windows and on the controls panel frame (inner margin 14); no border on inactive widgets; plot colours left `#467DD7`, right `#D25A5A`, normal-hearing band green at ~11 % opacity; stock egui light and dark visuals otherwise. The segmented control looks like today's SwiftUI one: a 7 pt rounded track in the faint background colour, the selected segment filled with the accent and white text. Plus the widgets that stock egui lacks to reach it: a segmented control (view tabs, Speech/Music/Auto, Left/Right), and the dB value labels next to slider titles. Light and dark follow the system appearance and change with it.
- **Languages**: today's `App/Localizable.xcstrings` and `AU/Localizable.xcstrings` are merged into one strings file in the same xcstrings JSON format under `desktop/`, read at build time, and looked up by the English source string, as today. The locale comes from the system. Existing translations are reused wherever the wording still fits. Strings that must change or are new (for example the engine-failure status, below) are translated into all seven languages as part of this work.
- **Accessibility**: egui's AccessKit support is turned on, so VoiceOver can read and use the controls. It will be weaker than SwiftUI's; that is accepted.

## macOS behaviour to keep

- Bundle ID `se.lindefors.audiocurve`, so the system's audio-capture permission and the login item carry over.
- `Info.plist`: `LSUIElement`, `NSAudioCaptureUsageDescription` and the version keys as today.
- Background mode on: no Dock icon unless the window is open; Bypass off at launch. Off: an ordinary app, Bypass on at launch, quits when the window closes. Launch from the icon or a Dock click/reopen opens the window; launch as a login item stays hidden.
- Quitting unmutes at once (route torn down before exit), also on SIGTERM.
- Pending volume snapshots are restored at launch and when a device comes back.
- **One-time migration from the Swift app's UserDefaults** (same bundle ID, so the same defaults domain): on first launch the Rust app reads the pending volume snapshots (`hearingTest.pendingVolume`), `enabled`, `backgroundProcessing` and `showInMenuBar`, takes them over, and removes the snapshot key. This keeps the guarantee that a hearing test's raised volume is restored even across the upgrade. Profiles are not migrated (they live in the AUv3, see the gap above).

## Engine failure (the route controller's "plugin died")

With the kernel in-process there is no plugin process to die, but the IO can still fail (render errors, the device vanishing under the IOProc). The core's route controller keeps its latch unchanged and the model maps it:

- A failed render or IO reported by the Audio backend is `plugin_died(route_id)`. "Reload the plugin" means: drop the kernel, create a new one with the current parameters, rebuild the route.
- A second failure within 10 s latches the failed state, as today. The status line then says the sound processing stopped, with a Try again button (today's retry), instead of "The AudioCurve plugin could not be loaded". This is a new string, translated as above.

## Verification

- `core` and `app` unit tests for the model logic, with the fake platform: output switching, profile per output, Bypass and background mode rules, hearing-test flow (start checks, pause of the route, volume restore, save and assign).
- UI snapshot tests with `egui_kittest`: each view in light and dark renders without panicking; `make screenshots` writes the README images from it.
- `docs/manual-test.md` is updated for the Rust app and run by hand on the Mac at the end of stage 2: system-wide processing, output switching, sleep/wake, device unplug mid-test, crash mid-test (volume restored at next launch), permission denied and granted, login item, both background modes, all eight languages render.
- The Swift AUv3 still loads in a host after the packaging change.

## Calls made (not discussed with the user)

- Two stages, the Swift app shipping until stage 2 is done.
- `eframe` with the `wgpu` renderer; `tray-icon`/`muda` for the menu bar; `objc2` crates for CoreAudio, AppKit and ServiceManagement. Any of these can be swapped by the builder for a good reason.
- The xcstrings JSON is kept as the strings format (read at build time) rather than converting to Fluent, so the existing translations and tooling carry over.
- Settings as a separate window rather than a section of the main window, as today.
- The menu lists the outputs twice (check items, then "Profile for …" submenus), because NSMenu cannot combine a toggle and a submenu in one item.
- The README's menu panel screenshots are removed instead of drawing a mock of the native menu.
- The theme values are written out above, taken from the prototype the user approved.
- A one-time migration of the Swift app's UserDefaults (pending volumes and three settings), so the volume-restore guarantee survives the upgrade.
- "Plugin died" becomes "engine failed": rebuild kernel and route, same latch, new status text. New or changed strings are translated as part of the work instead of reusing only existing ones.

## Risks

- **winit and macOS app lifecycle.** eframe/winit has to support: no Dock icon with a window that can open and close, reopen on Dock click, staying alive with no window, opening a window from the tray. If winit cannot do one of these, the builder adds a small `objc2` shim in `platform/macos`. A short spike at the start of stage 2 checks this before the rest is built.
- **Process tap from Rust.** `CATapDescription` is an Objective-C class; the `objc2-core-audio` bindings may lack parts, needing hand-written bindings.

## Left to the builder

- Module and type names, the exact trait split.
- egui version and crate versions.
- How the model and the UI share state (one struct with methods, messages, or similar).
