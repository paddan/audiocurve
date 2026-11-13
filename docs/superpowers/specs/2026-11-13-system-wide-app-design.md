# AudioCurve 2: system-wide processing without a separate host

Date: 2026-11-13
Status: draft for review

## 1. Goal

AudioCurve 1.x is an AUv3 effect. To process all of the Mac's sound it needs a third-party host such as SoundSource. Version 2 makes `AudioCurve.app` a menu bar app that captures system audio itself, runs it through the AudioCurve plugin and plays it on the current output device.

Success means:

- With SoundSource uninstalled, everything the Mac plays is processed with the active profile when AirPods are the output.
- "Active on" works as today: on an output that is not ticked (for example hearing aids), the sound is not processed and does not pass through AudioCurve at all.
- The AU plugin keeps working in SoundSource, Audio Hijack and DAWs, with the same profiles and settings.
- A failure never leaves the Mac silent.

Out of scope, as separate later sub-projects: clearer speech (noise reduction, speech enhancement) and a built-in hearing test.

## 2. Approach

The app hosts the existing AudioCurve AUv3 plugin and feeds it with a Core Audio process tap (macOS 14.2 and later; AudioCurve requires macOS 27).

Rejected alternatives:

- Running the Rust kernel directly in the app. It needs the UI moved into a shared package and the profiles moved out of the plugin's container, which an unsandboxed app cannot read without a privacy prompt. Much more work for about 5–10 ms less latency.
- A virtual audio device (HAL driver). It needs an installer with an admin password and works poorly with ad hoc signing.

## 3. Audio path

```
all processes except AudioCurve
        │  process tap (muted while tapped)
        ▼
private aggregate device ──────────────► current output device
   input: the tap                          (main subdevice)
        │  IOProc (real-time thread)
        ▼
AudioCurve AUv3 (out of process, renderBlock) ──► IOProc output buffers
```

1. **Tap.** `CATapDescription` as a stereo global tap that excludes the app's own process, with `muteBehavior = .mutedWhenTapped`. Created with `AudioHardwareCreateProcessTap`.
2. **Aggregate device.** Created with `AudioHardwareCreateAggregateDevice`: private, the current default output device as main subdevice and the tap in `kAudioAggregateDeviceTapListKey`, with tap auto-start on. Its sample rate follows the output device.
3. **Render.** An IOProc on the aggregate device (`AudioDeviceCreateIOProcIDWithBlock`). For each cycle it calls the plugin's `renderBlock` with a pull-input block that hands over the tap's input buffer, and writes the result into the output buffers. All buffers are allocated before start; the IOProc does not allocate, lock or log.
4. **Plugin.** The same component as today (`aufx` / `hcmp` / `Lnds`), loaded out of process with `AVAudioUnit.instantiate(..., options: .loadOutOfProcess)`. Its profile, parameters and "Active on" selection are the plugin's own, stored where they are today, so the app and SoundSource share them. The app saves the plugin's parameter state (`fullState`) between launches.
5. **Formats.** Stereo float32 non-interleaved at the output device's rate. The plugin already handles 16–96 kHz.

### 3.1 Active on and output changes

A small state machine, `RouteController`, owns the tap, the aggregate device and the IOProc. Its inputs are the current default output device (from the existing `OutputDeviceMonitor`), the "Active on" selection, the on/off switch, the capture permission state and the plugin's load state. Its states:

| State | Meaning | Audio |
|---|---|---|
| `off` | The user turned AudioCurve off | System plays normally |
| `paused` | The current output is not ticked in "Active on" | System plays normally |
| `running(device)` | Tap and aggregate device built for `device` | Processed |
| `starting` | The plugin is loading | System plays normally |
| `failed(reason)` | Permission missing, plugin failed to load, or a rebuild failed twice | System plays normally |

On every change of the inputs the controller tears down what exists (IOProc, aggregate device, tap, in that order) and builds what the new state needs. A teardown followed by a build gives a short gap in the sound; the plugin's activity crossfade means no click. Rebuilds are coalesced: several device notifications within 200 ms lead to one rebuild.

"Active on" in the app means: tap on or off. The plugin's own pass-through keeps working for other hosts and stays consistent, since both read the same selection.

### 3.2 Latency

Expected total, to be measured: the output device's buffer, plus one IOProc cycle, plus the plugin's 2 ms lookahead; about 10–20 ms. The IOProc buffer is set to the smallest size the output device accepts at or above 256 frames.

## 4. App

### 4.1 Menu bar

A `MenuBarExtra` with the ear from the app icon as a template image:

- **Filled:** running.
- **Outline:** paused.
- **Crossed out:** off or failed.

Clicking it opens a panel (window-style `MenuBarExtra`) with:

1. An on/off switch for AudioCurve.
2. A status line: "Active – <device>", "Paused – <device> is the output", or the failure reason.
3. Profile: a picker over the saved profiles. The menu bar talks to the hosted plugin through a small AU interface (see 4.4).
4. Active on: one checkbox per output device, the same rules as in the plugin (`ActiveOutputs.setEnabled`, the last ticked box cannot be unticked).
5. "Open AudioCurve…", "Launch at login" (checkbox), "Quit AudioCurve".

### 4.2 Window

"Open AudioCurve…" opens one window with the plugin's own view (its `requestViewController`), as the test host does today, plus a toolbar with **Bypass (A/B)** and the status line. The file player (Open Audio File…, Play) is removed. The app is an accessory app (`LSUIElement`) and shows a Dock icon only while the window is open.

### 4.3 Launch and persistence

- "Launch at login" uses `SMAppService.mainApp`. It is off by default; the first launch asks once.
- The app remembers on/off and the plugin's parameter state in its own `UserDefaults`. Profiles and "Active on" stay in the plugin's store.
- The app needs `NSAudioCaptureUsageDescription` in `Info.plist`. It stays unsandboxed. The plugin stays sandboxed.

### 4.4 App ↔ plugin interface

The menu bar must show and change the active profile and "Active on" without the window open. The app cannot read the plugin's sandbox container, so it goes through the plugin's `fullState`, which crosses the out-of-process boundary in both directions (`AUMessageChannel` was tried and returns empty replies across processes; see the spike notes in `docs/superpowers/plans/2026-11-13-spike-notes.md`):

- Reading: the plugin's `fullState` getter includes a status value with the saved profiles (ID and name), the active profile's ID, the "Active on" selection and the hosts from 4.5 that are processing right now.
- Commands: the app sets `fullState` to a dictionary holding only a command (`loadProfile(id)` or `setActiveOutputs(selection)`). The plugin runs it and leaves its parameters and profile otherwise untouched.

### 4.5 Double processing

Each plugin instance writes a heartbeat file to its container (`instances/<pid>.json`: host bundle ID, active flag, time) every 2 s and removes it on deallocation. The app's plugin instance reads the other files and reports them in its status (4.4). If another live host is processing, the panel shows "AudioCurve is also running in <host>: the sound is processed twice."

## 5. Errors

| Situation | Behavior |
|---|---|
| Capture permission not granted or denied | `failed(permission)`: crossed-out icon, an explanation and a button that opens System Settings › Privacy & Security › Screen & System Audio Recording. Nothing is muted. |
| Plugin fails to load (including the stale `arkaudiod` registration seen in 1.0.x) | `failed(plugin)`: no tap is created, so the sound plays normally. The panel shows the error. |
| Plugin process dies while running | The render call returns an error: the controller tears down at once (unmutes), reloads the plugin and rebuilds once. A second failure within 10 s gives `failed(plugin)`. |
| Output device changes or disappears | Teardown and rebuild for the new device, or `paused` if it is not ticked. |
| Output sample rate changes | Teardown, plugin re-initialized at the new rate, rebuild. |
| App quits or crashes | Core Audio removes a process's taps and private aggregate devices when it exits, so system sound returns without the app. The app also tears down on `applicationWillTerminate`. |
| Aggregate device or tap creation fails | Logged with the `OSStatus`; one retry after 500 ms, then `failed(route)`. |

Rule: every failure path ends with the tap destroyed, so the Mac is never silent.

## 6. Code structure

- `App/` becomes the menu bar app: `AudioCurveApp` (scenes), `MenuPanel`, `MainWindow`, `AppModel` (on/off, launch at login, status).
- New `App/Audio/`: `ProcessTap` (create/destroy a tap), `AggregateDevice` (create/destroy), `RenderLoop` (IOProc and the AU render call), `RouteController` (the state machine, which depends on protocols so it can be tested with fakes).
- `PlayerEngine` and the file player in `HostView` are removed.
- `AudioCurveCore` gains `RouteDecision`: a pure function from (on/off, permission, plugin state, current output, Active on) to the target state. It holds the logic worth unit-testing.
- The plugin gains the interface from 4.4 and the heartbeat from 4.5.

## 7. Tests

Automatic:

- `RouteDecision` for every combination of inputs in 3.1, including an unknown output (no device to build for: `failed(route)`, so the sound plays normally).
- `RouteController` with fake tap, aggregate device and plugin: teardown order, coalescing of notifications, one retry, unmute on every failure path.
- Heartbeat parsing and staleness (older than 6 s means gone).
- The existing Rust and Swift tests stay green.

Manual, added to `docs/manual-test.md`:

- AirPods as output: music, a video and a call app are processed. Bypass gives an audible A/B.
- Switch to hearing aids and back: paused, then running, with no click and no lasting silence.
- Deny the capture permission, then grant it.
- Kill the `AudioCurveAU` process while audio plays: the sound continues unprocessed within a second, then processing resumes.
- Output at 44.1, 48 and 96 kHz.
- Latency: a video with speech has no visible lip-sync error; measure with a loopback recording and note the figure.
- One hour of mixed listening without dropouts.
- SoundSource also running AudioCurve: the warning appears.
- Quit the app while audio plays: the sound continues unprocessed.

## 8. Release

Version 2.0.0. The release workflow is unchanged apart from the new `Info.plist` key. The README describes the menu bar app as the main way to use AudioCurve, and SoundSource and other hosts as optional.
