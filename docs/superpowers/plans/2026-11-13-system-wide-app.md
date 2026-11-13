# AudioCurve 2 (system-wide app) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Turn `AudioCurve.app` into a menu bar app that captures system audio with a Core Audio process tap, runs it through the AudioCurve AUv3 plugin and plays it on the current output device.

**Architecture:** Pure decision logic (`RouteDecision`), the route state machine (`RouteController`), the heartbeat model and the app↔plugin message codec live in `AudioCurveCore` and are unit-tested with fakes. The Core Audio parts (tap, aggregate device, IOProc render loop) and the plugin host live in `App/Audio/`. The plugin gains an `AUMessageChannel` and a heartbeat writer. The SwiftUI app becomes a `MenuBarExtra` plus one window.

**Tech Stack:** Swift 6, SwiftUI, Core Audio HAL (process taps, aggregate devices), AVFoundation/AudioToolbox (AUv3 hosting), ServiceManagement, Swift Testing, XcodeGen.

**Spec:** `docs/superpowers/specs/2026-11-13-system-wide-app-design.md`

## Global Constraints

- macOS 27.0 deployment target, Apple silicon only (as today).
- Bundle IDs unchanged: `se.lindefors.audiocurve` (app), `se.lindefors.audiocurve.AudioCurveAU` (plugin). Component `aufx` / `hcmp` / `Lnds` unchanged.
- The plugin stays sandboxed; its profile store path stays `Application Support/audiocurve` in its container. The app stays unsandboxed.
- Every failure path ends with the tap destroyed: the Mac is never silent because of AudioCurve.
- The IOProc does not allocate, lock or log.
- User-facing text in English source strings, translated in the `.xcstrings` catalogs into sv, de, es, fr, fi, nb, da. Docs and screenshots contain no Swedish.
- Never use the user's real audiogram in code, tests, docs or screenshots; use `TestFixtures.exampleProfile()`.
- Commits use the noreply author already configured; end messages with the Co-Authored-By line.
- Build: `xcodegen generate && xcodebuild -scheme audiocurve -configuration Debug -derivedDataPath build/dd build`. Core tests: `cd Packages/AudioCurveCore && swift test`. Rust tests: `cd dsp && ~/.cargo/bin/cargo test`.
- Version 2.0.0 (`MARKETING_VERSION`), set only in Task 9.

## Review Focus

1. The output device disappears mid-render (AirPods taken out): the route must be torn down and rebuilt or paused, never left with a dead aggregate device and the system muted. → Task 4 test `deviceLossTearsDownBeforeRebuild`.
2. Several device notifications in a burst (Bluetooth connect fires default-device and device-list changes): exactly one rebuild. → Task 4 test `burstOfChangesCoalescesIntoOneBuild`.
3. The plugin process dies twice in a row: no rebuild loop, ends in `failed(.plugin)` with the tap gone. → Task 4 test `secondPluginDeathWithinTenSecondsFails`.
4. A stale heartbeat from a crashed SoundSource instance must not warn about double processing forever. → Task 3 test `staleAndInactiveBeatsAreIgnored`.
5. A status or command from an older or newer plugin that does not decode must not crash either side. → Task 5 test `unknownOrBadCommandIsNil`.

---

### Task 1: Spike — `AUMessageChannel` across the process boundary and permission detection (throwaway)

**Files:**
- Create (throwaway, not committed): a scratch branch `spike/message-channel` with edits to `AU/AudioCurveAudioUnit.swift` and `App/HostView.swift`.
- Create: `docs/superpowers/plans/2026-11-13-spike-notes.md` (committed on master).

**Interfaces:**
- Produces: a written answer to three questions, which Tasks 5–8 rely on.

- [ ] **Step 1:** In the plugin, override `messageChannel(for channelName: String) -> AUMessageChannel` to return an object whose `callAudioUnit(_ message: [AnyHashable: Any]) -> [AnyHashable: Any]` echoes `["echo": message["ping"] ?? "none", "pid": getpid()]` for channel name `"audiocurve"`.
- [ ] **Step 2:** In the host app (which loads the plugin with `.loadOutOfProcess`), call `auAudioUnit.messageChannel(for: "audiocurve").callAudioUnit?(["ping": "hello"])` and print the reply. Answer Q1: does the reply arrive, and is `pid` the `AudioCurveAU` process (not the app)?
- [ ] **Step 3:** Create a minimal tap (`CATapDescription(stereoGlobalTapButExcludeProcesses: [])`, `AudioHardwareCreateProcessTap`) in the app, start an IOProc on a private aggregate device with it, play music, and log the peak of the input buffers. Then reset with `tccutil reset AudioCapture se.lindefors.audiocurve`, deny the prompt and repeat. Answer Q2: when permission is denied, does tap creation fail with an `OSStatus`, or does it succeed and deliver silence? Answer Q3: what is the measured IOProc buffer size at 48 kHz on AirPods and on the built-in speakers?
- [ ] **Step 4:** Write the three answers in `docs/superpowers/plans/2026-11-13-spike-notes.md`. If Q1 fails, the note says "fallback: profile index parameter, Active on only in the window", and Tasks 5 and 8 use the fallback described there. If Q2 is "silence", the note says permission is detected as: no non-zero input sample for 3 s while `kAudioDevicePropertyDeviceIsRunningSomewhere` is true for the output device.
- [ ] **Step 5:** Delete the spike branch. Commit only the notes file.

```bash
git add docs/superpowers/plans/2026-11-13-spike-notes.md
git commit -m "docs: spike notes for AudioCurve 2"
```

### Task 2: `RouteDecision`

**Files:**
- Create: `Packages/AudioCurveCore/Sources/AudioCurveCore/RouteDecision.swift`
- Test: `Packages/AudioCurveCore/Tests/AudioCurveCoreTests/RouteDecisionTests.swift`

**Interfaces:**
- Consumes: `ActiveOutputs` (existing; `isActive(currentUID:)`).
- Produces:

```swift
public enum PermissionState: Sendable, Equatable { case granted, denied, unknown }
public enum PluginLoadState: Sendable, Equatable { case loading, loaded, failed }
public enum RouteFailure: Sendable, Equatable { case permission, plugin, route }
public enum RouteTarget: Sendable, Equatable {
    case off, starting, paused(deviceUID: String), running(deviceUID: String), failed(RouteFailure)
}
public struct RouteInputs: Sendable, Equatable {
    public var enabled: Bool
    public var permission: PermissionState
    public var plugin: PluginLoadState
    public var outputUID: String?
    public var activeOutputs: ActiveOutputs
}
public func routeTarget(for inputs: RouteInputs) -> RouteTarget
```

Precedence, first match wins: `!enabled` → `.off`; `permission == .denied` → `.failed(.permission)`; `plugin == .failed` → `.failed(.plugin)`; `plugin == .loading` → `.starting`; `outputUID == nil` → `.failed(.route)`; output not active → `.paused`; else `.running`. `permission == .unknown` counts as allowed (the first tap triggers the system prompt).

- [ ] **Step 1: Write the failing tests**

```swift
private func inputs(enabled: Bool = true, permission: PermissionState = .granted,
                    plugin: PluginLoadState = .loaded, output: String? = "airpods",
                    active: ActiveOutputs = ActiveOutputs()) -> RouteInputs { … }

@Test func offWinsOverEverything() { #expect(routeTarget(for: inputs(enabled: false, permission: .denied, plugin: .failed)) == .off) }
@Test func deniedPermissionFails() { #expect(routeTarget(for: inputs(permission: .denied)) == .failed(.permission)) }
@Test func unknownPermissionStillRuns() { #expect(routeTarget(for: inputs(permission: .unknown)) == .running(deviceUID: "airpods")) }
@Test func pluginStates() {
    #expect(routeTarget(for: inputs(plugin: .loading)) == .starting)
    #expect(routeTarget(for: inputs(plugin: .failed)) == .failed(.plugin))
}
@Test func unknownOutputFailsRoute() { #expect(routeTarget(for: inputs(output: nil)) == .failed(.route)) }
@Test func untickedOutputPauses() {
    let a = ActiveOutputs(devices: [OutputDevice(uid: "airpods", name: "AirPods Pro")])
    #expect(routeTarget(for: inputs(output: "aids", active: a)) == .paused(deviceUID: "aids"))
    #expect(routeTarget(for: inputs(output: "airpods", active: a)) == .running(deviceUID: "airpods"))
}
@Test func emptySelectionRunsEverywhere() { #expect(routeTarget(for: inputs(output: "spk")) == .running(deviceUID: "spk")) }
```

- [ ] **Step 2:** Run `cd Packages/AudioCurveCore && swift test --filter RouteDecision`. Expected: FAIL, `routeTarget` not defined.
- [ ] **Step 3:** Implement the types and `routeTarget(for:)` with the precedence above. `RouteInputs` gets a public memberwise init.
- [ ] **Step 4:** Run the same command. Expected: all pass. Then `swift test`: all pass.
- [ ] **Step 5:** Commit: `feat(core): route decision for the system-wide app`.

### Task 3: Instance heartbeats

**Files:**
- Create: `Packages/AudioCurveCore/Sources/AudioCurveCore/InstanceHeartbeat.swift`
- Modify: `Packages/AudioCurveCore/Sources/AudioCurveCore/ProfileStore.swift` (heartbeat I/O under `root/instances/`)
- Test: `Packages/AudioCurveCore/Tests/AudioCurveCoreTests/InstanceHeartbeatTests.swift`

**Interfaces:**
- Produces:

```swift
public struct InstanceHeartbeat: Codable, Sendable, Equatable {
    public var pid: Int32
    public var hostBundleID: String
    public var active: Bool      // processing on for the current output
    public var time: Date
    public static let interval: TimeInterval = 2
    public static let staleAfter: TimeInterval = 6
    /// Active beats newer than `staleAfter`, other than `pid`, sorted by host bundle ID.
    public static func live(_ beats: [InstanceHeartbeat], now: Date, excludingPID pid: Int32) -> [InstanceHeartbeat]
}
extension ProfileStore {
    public func writeHeartbeat(_ b: InstanceHeartbeat)      // instances/<pid>.json, atomic
    public func readHeartbeats() -> [InstanceHeartbeat]      // skips unreadable files
    public func removeHeartbeat(pid: Int32)
}
```

- [ ] **Step 1: Write the failing tests**

```swift
@Test func staleAndInactiveBeatsAreIgnored() {
    let now = Date(timeIntervalSince1970: 1000)
    let beats = [
        InstanceHeartbeat(pid: 1, hostBundleID: "com.rogueamoeba.soundsource", active: true, time: now.addingTimeInterval(-2)),
        InstanceHeartbeat(pid: 2, hostBundleID: "com.example.old", active: true, time: now.addingTimeInterval(-7)),
        InstanceHeartbeat(pid: 3, hostBundleID: "com.example.idle", active: false, time: now),
        InstanceHeartbeat(pid: 4, hostBundleID: "se.lindefors.audiocurve", active: true, time: now),
    ]
    #expect(InstanceHeartbeat.live(beats, now: now, excludingPID: 4).map(\.pid) == [1])
}
@Test func heartbeatsRoundTripThroughTheStore() throws {
    // temp root; write pid 10 and 11; remove 10; read → [11]; a "garbage.json" file is skipped.
}
```

- [ ] **Step 2:** Run `swift test --filter InstanceHeartbeat`. Expected: FAIL.
- [ ] **Step 3:** Implement. JSON with ISO-8601 dates, same encoder settings as the profile store.
- [ ] **Step 4:** Run `swift test`. Expected: all pass.
- [ ] **Step 5:** Commit: `feat(core): instance heartbeats`.

### Task 4: `RouteController`

**Files:**
- Create: `Packages/AudioCurveCore/Sources/AudioCurveCore/RouteController.swift`
- Test: `Packages/AudioCurveCore/Tests/AudioCurveCoreTests/RouteControllerTests.swift`

**Interfaces:**
- Consumes: `RouteInputs`, `RouteTarget`, `routeTarget(for:)` from Task 2.
- Produces:

```swift
/// One built route. The controller calls the three teardown steps in this order.
public struct BuiltRoute {
    public var stopIO: () -> Void
    public var destroyAggregate: () -> Void
    public var destroyTap: () -> Void
}
public protocol RouteBuilding: AnyObject {
    /// Creates tap, aggregate device and IOProc for the device and starts IO.
    func build(deviceUID: String) throws -> BuiltRoute
}
@MainActor public final class RouteController {
    public init(builder: RouteBuilding,
                now: @escaping () -> Date = Date.init,
                after: @escaping (TimeInterval, @escaping @MainActor () -> Void) -> Void)
    public var inputs: RouteInputs          // setting it schedules an apply after `coalesce`
    public private(set) var state: RouteTarget
    public var onStateChange: ((RouteTarget) -> Void)?
    public var onReloadPlugin: (() -> Void)?
    public static let coalesce: TimeInterval = 0.2
    public static let retryDelay: TimeInterval = 0.5
    public static let deathWindow: TimeInterval = 10
    /// The render loop reported a failed render (plugin process gone). Tears down at once.
    public func pluginDied()
    /// Forces a teardown and rebuild after `coalesce`, even if the target is unchanged
    /// (the output device's sample rate changed).
    public func invalidate()
    /// Tears down at once (app quitting).
    public func shutDown()
}
```

Behavior:
- Applying: compute the target. If it equals `state` and a route exists for that device, do nothing. Otherwise tear down any existing route (in `BuiltRoute` order), then build only if the target is `.running`. Set `state` and call `onStateChange` once per change.
- Build throws: `state` stays torn down, retry once after `retryDelay`; a second throw gives `.failed(.route)` until inputs change.
- `pluginDied()`: tear down immediately (no coalescing), set `inputs.plugin = .loading`, call `onReloadPlugin`. If the previous death was within `deathWindow`, set `inputs.plugin = .failed` instead and do not call `onReloadPlugin`.
- `shutDown()`: tear down immediately, `state = .off`.

- [ ] **Step 1: Write the failing tests** with a `FakeBuilder` that records calls into a shared `[String]` log (`"build:<uid>"`, `"stopIO"`, `"destroyAggregate"`, `"destroyTap"`) and can be told to throw, and a manual `after` that queues closures which the test runs with `fire()`.

```swift
@Test func runningBuildsOnceAfterCoalescing()            // set inputs → no build before fire(); after fire(): ["build:airpods"], state .running
@Test func burstOfChangesCoalescesIntoOneBuild()         // set inputs 3× (outputs a, b, airpods) before fire() → one "build:airpods"
@Test func deviceLossTearsDownBeforeRebuild()            // running on airpods → output "spk" → log tail == ["stopIO","destroyAggregate","destroyTap","build:spk"]
@Test func pausingTearsDownAndDoesNotBuild()             // running → untick airpods → tail == ["stopIO","destroyAggregate","destroyTap"], state .paused
@Test func buildFailureRetriesOnceThenFails()            // builder throws twice → two build attempts, state .failed(.route), no route held
@Test func pluginDeathTearsDownImmediatelyAndReloads()   // running → pluginDied() → teardown in log without fire(), onReloadPlugin called, inputs.plugin == .loading
@Test func secondPluginDeathWithinTenSecondsFails()      // now = t, die; now = t+5, die → state .failed(.plugin), onReloadPlugin called once
@Test func unchangedTargetDoesNotRebuild()               // running, set same inputs again, fire() → no new build
@Test func invalidateRebuildsSameDevice()           // running on airpods → invalidate(), fire() → tail == ["stopIO","destroyAggregate","destroyTap","build:airpods"]
@Test func shutDownTearsDown()
```

- [ ] **Step 2:** Run `swift test --filter RouteController`. Expected: FAIL.
- [ ] **Step 3:** Implement as specified.
- [ ] **Step 4:** Run `swift test`. Expected: all pass.
- [ ] **Step 5:** Commit: `feat(core): route controller`.

### Task 5: Plugin side — status/command channel over `fullState`, and heartbeat

(Revised after the spike: `AUMessageChannel` returns empty replies across processes; `fullState` works both ways. See `docs/superpowers/plans/2026-11-13-spike-notes.md`.)

**Files:**
- Create: `Packages/AudioCurveCore/Sources/AudioCurveCore/PluginChannel.swift`
- Test: `Packages/AudioCurveCore/Tests/AudioCurveCoreTests/PluginChannelTests.swift`
- Modify: `AU/AudioCurveAudioUnit.swift` (status in the `fullState` getter, command handling in the setter, heartbeat timer, `loadSaved(id:)`)
- Modify: `AU/PluginViewModel.swift` (`load(_:)` uses `audioUnit.loadSaved(id:)`)

**Interfaces:**
- Consumes: `InstanceHeartbeat`, `ProfileStore` heartbeat I/O (Task 3).
- Produces:

```swift
public struct ProfileSummary: Codable, Sendable, Equatable { public var id: UUID; public var name: String }
public struct PluginStatus: Codable, Sendable, Equatable {
    public var profiles: [ProfileSummary]
    public var activeProfileID: UUID?
    public var activeOutputs: ActiveOutputs
    public var otherHosts: [String]          // host bundle IDs processing right now
}
public enum PluginCommand: Codable, Sendable, Equatable {
    case loadProfile(UUID)
    case setActiveOutputs(ActiveOutputs)
    case identifyHost(String)
}
public enum PluginChannel {
    public static let statusKey = "audiocurve.status"
    public static let commandKey = "audiocurve.command"
    public static func encode(_ status: PluginStatus) -> Data
    public static func status(from fullState: [String: Any]?) -> PluginStatus?   // nil if missing or undecodable
    public static func commandState(_ command: PluginCommand) -> [String: Any]   // [commandKey: Data]
    public static func command(from fullState: [String: Any]?) -> PluginCommand? // nil if missing, unknown or undecodable
}
```

Plugin: the `fullState` getter adds `[PluginChannel.statusKey: PluginChannel.encode(status)]`, where `otherHosts` = host bundle IDs of `InstanceHeartbeat.live(store.readHeartbeats(), now: Date(), excludingPID: getpid())`. The setter first checks `PluginChannel.command(from:)`: if it is non-nil, run it on the main queue (`loadSaved(id:)` or `setActiveOutputs(_:)`) and return without calling `super.fullState` or touching the profile. A dictionary with `commandKey` that does not decode is ignored entirely (also returns early). `loadSaved(id:)` moves the existing load logic from `PluginViewModel.load(_:)` into the audio unit and notifies the view model via `onProfileChange`. Heartbeat: a main-queue timer every `InstanceHeartbeat.interval` writes `InstanceHeartbeat(pid: getpid(), hostBundleID: <host>, active: isProcessingActive, time: Date())`; `deinit` removes it. A plugin cannot see which app hosts it, so `hostBundleID` is `"unknown"` until the host sends `.identifyHost(bundleID)`; the AudioCurve app sends it right after loading the plugin. The panel shows an unknown host as "another app".

- [ ] **Step 1: Write the failing tests**

```swift
@Test func statusRoundTrips() {
    let s = PluginStatus(profiles: [ProfileSummary(id: UUID(), name: "Example")], activeProfileID: nil,
                         activeOutputs: ActiveOutputs(devices: [OutputDevice(uid: "a", name: "A")]), otherHosts: ["com.example.host"])
    #expect(PluginChannel.status(from: [PluginChannel.statusKey: PluginChannel.encode(s), "other": 1]) == s)
}
@Test func commandsRoundTrip() {
    for c: PluginCommand in [.loadProfile(UUID()), .setActiveOutputs(ActiveOutputs()), .identifyHost("se.lindefors.audiocurve")] {
        #expect(PluginChannel.command(from: PluginChannel.commandState(c)) == c)
    }
}
@Test func unknownOrBadCommandIsNil() {
    #expect(PluginChannel.command(from: nil) == nil)
    #expect(PluginChannel.command(from: ["data": 1]) == nil)
    #expect(PluginChannel.command(from: [PluginChannel.commandKey: Data("{\"launchMissiles\":{}}".utf8)]) == nil)
    #expect(PluginChannel.status(from: [PluginChannel.statusKey: "nope"]) == nil)
}
```

- [ ] **Step 2:** Run `swift test --filter PluginChannel`. Expected: FAIL.
- [ ] **Step 3:** Implement the codec (JSON), then the plugin side.
- [ ] **Step 4:** `swift test` passes; the build succeeds. Manual (following the developer note in the spike notes): loading a profile in the plugin UI still works; the `instances/` folder in the plugin container has one file while the host app runs and none a few seconds after it quits.
- [ ] **Step 5:** Commit: `feat(au): status and commands over fullState, instance heartbeat`.

### Task 6: Core Audio route (tap, aggregate device, render loop)

**Files:**
- Create: `App/Audio/ProcessTap.swift`, `App/Audio/AggregateDevice.swift`, `App/Audio/RenderLoop.swift`, `App/Audio/CoreAudioRouteBuilder.swift`
- Modify: `App/Info.plist` (`NSAudioCaptureUsageDescription`: "AudioCurve processes the sound your Mac plays so it matches your hearing.")

**Interfaces:**
- Consumes: `RouteBuilding`, `BuiltRoute` (Task 4); the plugin's `AUAudioUnit` (Task 7 passes it in).
- Produces:

```swift
final class ProcessTap { init(excluding pids: [pid_t]) throws; let id: AudioObjectID; let uid: String; func destroy() }
final class AggregateDevice { init(outputUID: String, tapUID: String) throws; let id: AudioObjectID; func destroy() }
final class RenderLoop {
    init(device: AudioObjectID, unit: AUAudioUnit, maxFrames: AUAudioFrameCount, onRenderFailure: @escaping @Sendable () -> Void) throws
    func start() throws; func stop()
}
@MainActor final class CoreAudioRouteBuilder: RouteBuilding {
    init(unit: @escaping () -> AUAudioUnit?, onRenderFailure: @escaping @Sendable () -> Void,
         onSampleRateChange: @escaping @MainActor () -> Void)
}
struct CoreAudioError: Error { let status: OSStatus; let call: String }
```

Values from the spec: tap is a stereo global tap excluding the app's own pid, `muteBehavior = .mutedWhenTapped`, private; aggregate device private, main subdevice = output UID, tap list = [tap UID] with drift compensation on, tap auto-start on; IOProc buffer = the smallest the device accepts that is ≥ 256 frames; stereo float32 non-interleaved at the output's nominal rate. `build(deviceUID:)` also sets the unit's input and output bus format to that rate and calls `allocateRenderResources()` when the rate changed. `RenderLoop` preallocates an `AudioBufferList` for `maxFrames`, passes the tap's input through the render block's pull-input, and on a non-zero render status writes silence for that cycle and calls `onRenderFailure` once (an atomic flag guards it). While a route exists, the builder listens to `kAudioDevicePropertyNominalSampleRate` on the output device and calls `onSampleRateChange` (wired to `RouteController.invalidate()` in Task 7). Teardown order inside `BuiltRoute`: `stopIO` = `RenderLoop.stop` + `AudioDeviceDestroyIOProcID`, then `AggregateDevice.destroy`, then `ProcessTap.destroy`.

- [ ] **Step 1:** Implement the four files. No unit tests (Core Audio objects); the controller logic is tested in Task 4.
- [ ] **Step 2:** Temporary manual harness on a scratch branch: in `HostView`, a button builds `CoreAudioRouteBuilder` with the loaded plugin for the current default output. Play music in another app. Expected: the sound is processed (Bypass in the window makes an audible difference), and stopping the route brings back unprocessed sound within a second. Measure the IOProc buffer size and log it once.
- [ ] **Step 3:** Remove the harness. `xcodebuild … build` succeeds.
- [ ] **Step 4:** Commit: `feat(app): core audio route for system-wide processing`.

### Task 7: Plugin host and app model

**Files:**
- Create: `App/PluginHost.swift`, `App/AppModel.swift`, `App/PluginClient.swift`
- Modify: `App/OutputDeviceMonitor` usage: move `AU/OutputDeviceMonitor.swift` into `AudioCurveCore` (`Sources/AudioCurveCore/OutputDeviceMonitor.swift`) so both targets use it; the plugin imports it from there.
- Delete: `App/PlayerEngine.swift`

**Interfaces:**
- Consumes: `RouteController`, `RouteInputs` (Tasks 2, 4), `CoreAudioRouteBuilder` (Task 6), `PluginChannel`/`PluginStatus`/`PluginCommand` (Task 5), `OutputDeviceMonitor` (existing: `current`, `available`, `onChange`).
- Produces:

```swift
@MainActor final class PluginHost {
    private(set) var unit: AVAudioUnit?
    var loadState: PluginLoadState { get }
    func load() async                      // .loadOutOfProcess, restores fullState from UserDefaults key "pluginState"
    func reload() async
    func saveState()                       // fullState → UserDefaults "pluginState" (property list data)
    var bypass: Bool { get set }           // auAudioUnit.shouldBypassEffect
}
@MainActor final class PluginClient {
    init(host: PluginHost)
    func status() -> PluginStatus?               // PluginChannel.status(from: unit.fullState)
    func send(_ command: PluginCommand)          // unit.fullState = PluginChannel.commandState(command)
}
@Observable @MainActor final class AppModel {
    var enabled: Bool                      // UserDefaults "enabled", default true
    private(set) var state: RouteTarget
    private(set) var currentOutput: OutputDevice?
    private(set) var availableOutputs: [OutputDevice]
    private(set) var activeOutputs: ActiveOutputs
    private(set) var profiles: [ProfileSummary]; private(set) var activeProfileID: UUID?
    private(set) var otherHosts: [String]  // polled every 2 s while the panel or window is open
    var launchAtLogin: Bool                // SMAppService.mainApp register/unregister
    func start() async                     // load plugin, start monitor, feed RouteController
    func setOutput(_ d: OutputDevice, enabled: Bool)   // ActiveOutputs.setEnabled + PluginClient.setActiveOutputs
    func loadProfile(_ id: UUID)
    func quit()                            // RouteController.shutDown, PluginHost.saveState, NSApp.terminate
}
```

`AppModel` refreshes `PluginClient.status()` every 2 s while the panel or window is open and after each command. `RouteController`'s build runs `AudioDeviceStart` off the main thread (it blocks while the permission prompt is up). `AppModel` builds `RouteInputs` from `enabled`, permission (per the spike notes: `.denied` on the detected failure, otherwise `.unknown` until audio has flowed, then `.granted`), `PluginHost.loadState`, `OutputDeviceMonitor.current?.uid` and `activeOutputs`; `RouteController.onReloadPlugin` calls `PluginHost.reload()`; `applicationWillTerminate` calls `quit()`'s teardown.

- [ ] **Step 1:** Move `OutputDeviceMonitor` into the core package; `swift test` and the build pass.
- [ ] **Step 2:** Implement `PluginHost`, `PluginClient`, `AppModel`. Remove `PlayerEngine` and the file player from `HostView` (the view keeps the plugin view for now).
- [ ] **Step 3:** Build. Manual: launch the app with music playing in another app. Expected: processed sound on AirPods; untick AirPods in the plugin view → unprocessed within a second; `kill` the `AudioCurveAU` process → sound continues unprocessed, processing resumes within a few seconds.
- [ ] **Step 4:** Commit: `feat(app): plugin host and app model drive the system-wide route`.

### Task 8: Menu bar panel and window

**Files:**
- Modify: `App/AudioCurveApp.swift` (scenes), `App/Info.plist` (`LSUIElement` = YES)
- Create: `App/MenuPanel.swift`, `App/MainWindow.swift`, `App/MenuBarIcon.swift`, `App/Assets/` template images `MenuBarOn`, `MenuBarPaused`, `MenuBarOff` (the ear from `App/AppIcon.icon/Assets/ear.svg`, outline variant and crossed-out variant)
- Delete: `App/HostView.swift`, `App/ViewState.swift` if unused
- Modify: `App/Localizable.xcstrings` (all new strings, 8 languages)

**Interfaces:**
- Consumes: `AppModel` (Task 7), `ActiveOutputs.isChecked`, `isLastChecked` (existing).
- Produces: the user-facing app.

Content, from spec 4.1–4.3:
- `MenuBarExtra(isInserted:)` with `.menuBarExtraStyle(.window)`; icon `MenuBarOn` for `.running`, `MenuBarPaused` for `.paused`/`.starting`, `MenuBarOff` for `.off`/`.failed`.
- Panel, top to bottom: title "AudioCurve" with a switch bound to `enabled`; status line ("Active – %@", "Paused – %@ is the output", "Starting…", "Off", "Allow AudioCurve to record system audio" with a button "Open System Settings" that opens `x-apple.systempreferences:com.apple.settings.PrivacySecurity.extension?Privacy_AudioCapture`, "The AudioCurve plugin could not be loaded", "The audio route could not be built"); when `otherHosts` is non-empty, an orange line "AudioCurve is also running in %@: the sound is processed twice." (app name from `NSWorkspace` for the bundle ID, or "another app" for `"unknown"`); "Profile" picker; "Active on" checkboxes (same rules and "(current)"/"(not connected)" labels as the plugin); "Open AudioCurve…", "Launch at login" checkbox, "Quit AudioCurve".
- `Window("AudioCurve", id: "main")`: toolbar with a "Bypass (A/B)" checkbox and the status line; content is the plugin's view controller (`auAudioUnit.requestViewController`) as `HostView` does today; minimum size 720×560. Opening it sets `NSApp.setActivationPolicy(.regular)`; closing the last window sets `.accessory`.
- First launch only (`UserDefaults` "askedLaunchAtLogin"): an alert "Start AudioCurve when you log in?" with "Start at Login" and "Not Now".

- [ ] **Step 1:** Implement the scenes, panel, window, icons and first-launch alert. Add every new string to `App/Localizable.xcstrings` with translations for sv, de, es, fr, fi, nb, da.
- [ ] **Step 2:** Build. Manual: the menu bar icon appears and changes between the three looks; the panel matches the spec list; Profile and Active on changes from the panel show up in the window's plugin view and vice versa; the Dock icon appears only while the window is open; `-AppleLanguages '(de)'` shows German in the panel.
- [ ] **Step 3:** Commit: `feat(app): menu bar panel and main window`.

### Task 9: Docs, manual tests and version

**Files:**
- Modify: `docs/manual-test.md` (new section "System-wide app (2.0)" with the manual list from spec §7)
- Modify: `README.md` (menu bar app as the main way to use AudioCurve; SoundSource and other hosts optional; the capture permission; new screenshots of the panel and window taken with `TestFixtures`-style example data, English UI)
- Modify: `project.yml` (`MARKETING_VERSION: 2.0.0`)

- [ ] **Step 1:** Write the docs. Screenshots use the example audiogram and English UI (`-AppleLanguages '(en)'`), no personal device names (rename or crop).
- [ ] **Step 2:** Full verification: `cd dsp && ~/.cargo/bin/cargo test`, `cd Packages/AudioCurveCore && swift test`, Release build with `xcodebuild -scheme audiocurve -configuration Release -derivedDataPath build/dd build`. Expected: all pass.
- [ ] **Step 3:** Commit: `docs: AudioCurve 2.0 manual tests and README; version 2.0.0`.
