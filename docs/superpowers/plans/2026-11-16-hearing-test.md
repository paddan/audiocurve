# AudioCurve 2.2 Hearing Test Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A thorough hearing test run through the user's own headphones, saved as a profile, plus profiles and level settings that follow the output device.

**Architecture:** Everything that can be pure lives in `AudioCurveCore` and is unit-tested: calibration and level caps, the test procedure as a state machine, the tone synthesis with its hard clamp, the measurement record, output assignments and the output-switch decision. The AU (which owns profiles and parameters) applies assignments and switches profiles; only the app's own AU instance switches. The app adds a tone generator on AVAudioEngine, device-volume save/restore, the test model and view, and pauses its route while testing.

**Tech Stack:** Swift 6, Swift Testing (`import Testing`), SwiftUI, AVAudioEngine, Core Audio HAL, XcodeGen, Makefile.

**Spec:** `docs/superpowers/specs/2026-11-16-hearing-test-design.md`

## Global Constraints

- macOS 27, Apple silicon, Swift 6 language mode. `make release` (no Xcode) and `xcodebuild -scheme audiocurve` must both keep building.
- The Makefile compiles only `App/*.swift`, `App/Audio/*.swift`, `AU/*.swift`, `AU/UI/*.swift` and `Packages/AudioCurveCore/Sources/AudioCurveCore/*.swift`: put new files in those folders (no subfolders). Run `xcodegen generate` after adding files.
- Views use `@ViewState`, never `@State` (the Makefile rejects it).
- English source strings; every new UI string gets all 7 translations (da, de, es, fi, fr, nb, sv) in the matching `Localizable.xcstrings` (`App/` or `AU/`).
- `AudioCurveCore` has no resources: localized text is passed in by the UI (pattern: `ProfileStore.defaultCopyName`).
- Profile file format stays `fileVersion` 1; new fields are optional so existing files still decode.
- Bundle IDs, the profile path and AU parameter addresses 0–7 do not change. App bundle ID: `se.lindefors.audiocurve`.
- Test tone caps (spec 4.5), never exceeded: 125–500 Hz −1 dBFS, 750–1500 Hz −3 dBFS, 2000–8000 Hz −5 dBFS. A sine at 0 dBFS has peak amplitude 1.0.
- Commits use `19973950+paddan@users.noreply.github.com`, conventional prefixes (`feat(core):`, `feat(au):`, `feat(app):`, `docs:`), and end with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`. No pushes or tags without the user's approval.
- Version 2.2.0 (`project.yml` `MARKETING_VERSION`), bumped in the last task.

## Review Focus

1. **Holding the space bar / key auto-repeat:** one press per tone counts; repeats (`isARepeat`) and a second press in the same window are ignored, never counted as false presses. Pinned in Task 2 (`extraResponsesIgnored`) and Task 9 (key handler ignores repeats).
2. **A parameter change debounced just before an output switch** must be saved to the output it was made on, not to the new one. Pinned in Task 6 (the pending level save captures the UID).
3. **Fresh install (no working copy) or a flat unsaved working copy when the output switches:** nothing is saved, the assigned profile just loads. Pinned in Task 4 (`noWorkingCopyJustLoads`, `flatUnsavedIsNotSaved`).
4. **Long device names** in the default result name: "<60-char name> – 16 Nov 2026" is truncated to 60 chars and the " (2)" suffix still fits and stays unique. Pinned in Task 3 (`uniqueNameTruncatesBeforeSuffix`).
5. **Normal hearing at the floor:** a listener who hears everything goes no lower than −10 dB HL and the threshold is −10, not a value below the profile range. Pinned in Task 2 (`floorIsMinus10`).

---

### Task 1: Test calibration and level caps

**Files:**
- Create: `Packages/AudioCurveCore/Sources/AudioCurveCore/TestCalibration.swift`
- Test: `Packages/AudioCurveCore/Tests/AudioCurveCoreTests/TestCalibrationTests.swift`

**Interfaces:**
- Produces:
  - `public enum Transducer: String, Codable, Sendable { case inEar, overEar }`
  - `public struct TestCalibration: Codable, Equatable, Sendable { var transducer: Transducer; var fullScaleSPL: Double /* default 100 */; var kind: Kind }` with `public enum Kind: String, Codable, Sendable { case nominal }` (room for a measured kind later).
  - `public static func nominal(_ t: Transducer) -> TestCalibration`
  - `public static func retspl(_ t: Transducer, _ frequencyHz: Double) -> Double`
  - `public static func capDBFS(_ frequencyHz: Double) -> Double`
  - `public func hl(fromDBFS: Double, at f: Double) -> Double` = `dBFS + fullScaleSPL − retspl(transducer, f)` (unrounded)
  - `public func dBFS(fromHL: Double, at f: Double) -> Double` (inverse)
  - `public func maxHL(at f: Double) -> Double` = `hl(fromDBFS: capDBFS(f))` floored to a multiple of 5
  - `public static func roundedHL(_ hl: Double) -> Double` = rounded to nearest 5, clamped to −10…120
  - `public static func defaultTransducer(deviceName: String) -> Transducer` (`.inEar` if the name contains "AirPods", case-insensitive; else `.overEar`)

- [ ] **Step 1: Write the failing tests**

```swift
@Test func capsPerFrequency() {
    for f in [125.0, 250, 500] { #expect(TestCalibration.capDBFS(f) == -1) }
    for f in [750.0, 1000, 1500] { #expect(TestCalibration.capDBFS(f) == -3) }
    for f in [2000.0, 3000, 4000, 6000, 8000] { #expect(TestCalibration.capDBFS(f) == -5) }
}
@Test func conversionRoundTrips() {
    let c = TestCalibration.nominal(.inEar)
    for f in Audiogram.standardFrequencies {
        #expect(abs(c.dBFS(fromHL: c.hl(fromDBFS: -50, at: f), at: f) + 50) < 1e-9)
        #expect(c.hl(fromDBFS: -60, at: f) == -60 + 100 - TestCalibration.retspl(.inEar, f))
    }
}
@Test func maxHLIsMultipleOf5AndUnderCap() {
    for t in [Transducer.inEar, .overEar] { let c = TestCalibration.nominal(t)
        for f in Audiogram.standardFrequencies {
            #expect(c.maxHL(at: f).truncatingRemainder(dividingBy: 5) == 0)
            #expect(c.dBFS(fromHL: c.maxHL(at: f), at: f) <= TestCalibration.capDBFS(f) + 1e-9)
        } }
}
@Test func retsplShape() {   // the tables differ, and both have their minimum in the mid range
    for t in [Transducer.inEar, .overEar] {
        #expect(TestCalibration.retspl(t, 125) > TestCalibration.retspl(t, 1000) + 10)
    }
    #expect(Audiogram.standardFrequencies.contains { TestCalibration.retspl(.inEar, $0) != TestCalibration.retspl(.overEar, $0) })
}
@Test func roundingAndClamp() {
    #expect(TestCalibration.roundedHL(42.4) == 40); #expect(TestCalibration.roundedHL(42.6) == 45)
    #expect(TestCalibration.roundedHL(-30) == -10); #expect(TestCalibration.roundedHL(140) == 120)
}
@Test func defaultTransducerFromName() {
    #expect(TestCalibration.defaultTransducer(deviceName: "Patrik’s AirPods Pro") == .inEar)
    #expect(TestCalibration.defaultTransducer(deviceName: "WH-1000XM6") == .overEar)
}
```

- [ ] **Step 2: Run** `cd Packages/AudioCurveCore && swift test --filter TestCalibrationTests` — expect FAIL (types missing).
- [ ] **Step 3: Implement `TestCalibration.swift`.** RETSPL tables for the 11 standard frequencies: in-ear from ISO 389-2 (insert earphone, occluded-ear simulator), over-ear from ISO 389-8 (circumaural). Look up the published values and cite the standard and table in a doc comment; frequencies not in the table are a programming error (`precondition`).
- [ ] **Step 4: Run** the same command — expect PASS.
- [ ] **Step 5: Commit** `feat(core): test calibration and per-frequency tone caps`.

---

### Task 2: Hearing test procedure (state machine)

**Files:**
- Create: `Packages/AudioCurveCore/Sources/AudioCurveCore/HearingTest.swift`
- Test: `Packages/AudioCurveCore/Tests/AudioCurveCoreTests/HearingTestTests.swift`

**Interfaces:**
- Consumes: `TestCalibration` (Task 1).
- Produces:
  - `public struct Presentation: Equatable, Sendable { var ear: Ear; var frequencyHz: Double; var levelHL: Double; var levelDBFS: Double?  /* nil = catch trial, silent */; var gapBeforeS: Double; var index: Int /* 1-based point number for progress */ }`
  - `public enum TestFlag: Codable, Equatable, Sendable { case retestMismatch(Ear, dB: Double), catchTrialPresses(Int), noResponseAtMax(Ear, frequencyHz: Double), notConverged(Ear, frequencyHz: Double), asymmetry }`
  - `public struct TestPoint: Codable, Equatable, Sendable { var frequencyHz: Double; var dBFS: Double; var hl: Double /* rounded */ }`
  - `public struct HearingTestResult: Equatable, Sendable { var left: [TestPoint]; var right: [TestPoint]  /* sorted by frequency, 11 each */; var flags: [TestFlag]; func audiogram(_ ear: Ear) -> Audiogram }`
  - `public enum HearingTestStep: Equatable, Sendable { case present(Presentation), finished(HearingTestResult) }`
  - `public struct HearingTest: Sendable` with
    - `public static let order: [Double] = [1000, 1500, 2000, 3000, 4000, 6000, 8000, 1000, 750, 500, 250, 125]`, ears `[.right, .left]`; `public static var totalPoints: Int { 24 }`
    - `public init(calibration: TestCalibration, random: @escaping @Sendable () -> Double)` — `random()` returns 0..<1; used for catch trials (`< 1/8`) and gaps (`1.5 + 2 * random()`).
    - `public mutating func next() -> HearingTestStep` — returns the current presentation (idempotent until answered) or the result.
    - `public mutating func respond(heard: Bool)` — answers the current presentation; a second call before `next()` is ignored.

**Procedure (spec 4.3), in HL steps of 5:** start 40 dB HL (or `maxHL` if lower). Familiarization: not heard → +20 (+5 once the level is above 80), capped at `maxHL`. After the first "heard": −10 after every heard, +5 after every not heard, floor −10. A presentation reached by a +5 step is *ascending*. Threshold = the first level with 2 ascending "heard". Two "not heard" at `maxHL` → threshold `maxHL`, flag `noResponseAtMax`. More than 12 non-catch presentations at one frequency → threshold = lowest ascending-heard level (else lowest heard, else `maxHL`), flag `notConverged`. Catch trials do not move the staircase; "heard" on one increments a counter, and more than 2 in the run flags `catchTrialPresses(n)`. Retest at 1 kHz: `|first − second| > 10` flags `retestMismatch(ear, dB:)`; the stored 1 kHz value is the lower of the two. At the end, any frequency with `|L − R| >= 40` adds one `asymmetry`. `TestPoint.dBFS` is the raw presented level of the threshold; `hl` is `TestCalibration.roundedHL`.

- [ ] **Step 1: Write the failing tests.** A test helper `simulate(thresholdHL: (Ear, Double) -> Double, random:)` answers `heard = levelDBFS != nil && levelHL >= threshold`.

```swift
@Test func findsSimulatedThresholds()        // listener 35 dB HL everywhere → every point hl == 35
@Test func floorIsMinus10()                   // hears everything → every hl == -10, no level presented below -10
@Test func neverExceedsCap()                  // hears nothing → every presented levelDBFS <= capDBFS(f); every point flagged noResponseAtMax; hl == roundedHL(maxHL)
@Test func familiarizationStepsAbove80()      // threshold 100 → after 80 the presented levels rise by 5
@Test func orderAndCount()                    // presentations' (ear, f) sequence follows order, right first; 24 distinct index values; result has 11 points per ear
@Test func catchTrialsAreSilentAndCounted()   // random always 0 for catch draws: levelDBFS == nil; answering heard on 3 catch trials → flags contains .catchTrialPresses(3)
@Test func retestMismatchFlagged()            // listener whose 1 kHz threshold is 20 the first time and 40 the second → .retestMismatch(.right, dB: 20); stored 1 kHz hl == 20
@Test func notConvergedBailsOut()             // alternating heard/not-heard at every level → .notConverged within 12 presentations per frequency
@Test func asymmetryFlagged()                 // right 10, left 60 → flags contains .asymmetry exactly once
@Test func extraResponsesIgnored()            // respond(heard: true) twice, then next(): the staircase moved one step only
@Test func nextIsIdempotent()                 // two next() calls without respond return the same presentation
```

- [ ] **Step 2: Run** `swift test --filter HearingTestTests` — expect FAIL.
- [ ] **Step 3: Implement `HearingTest.swift`** per the procedure above.
- [ ] **Step 4: Run** — expect PASS.
- [ ] **Step 5: Commit** `feat(core): hearing test procedure`.

---

### Task 3: Tone synthesis with hard clamp

**Files:**
- Create: `Packages/AudioCurveCore/Sources/AudioCurveCore/ToneSynth.swift`
- Test: `Packages/AudioCurveCore/Tests/AudioCurveCoreTests/ToneSynthTests.swift`

**Interfaces:**
- Consumes: `TestCalibration.capDBFS`, `Ear`.
- Produces:
  - `public struct ToneSynth: Sendable` — realtime-safe value type (no allocation in `render`).
    - `public static let pulseOn = 0.2, pulseOff = 0.2, pulses = 3, ramp = 0.02; public static var duration: Double { 1.0 }` (3 on + 2 off)
    - `public init(sampleRate: Double)`
    - `public mutating func start(ear: Ear, frequencyHz: Double, levelDBFS: Double?)` — nil = silent tone of the same duration
    - `public mutating func render(left: UnsafeMutablePointer<Float>, right: UnsafeMutablePointer<Float>, frames: Int)` — writes the tone into the chosen ear's channel, zeros elsewhere; after `duration`, silence.
    - `public var isFinished: Bool`

- [ ] **Step 1: Write the failing tests**

```swift
@Test func clampsToCapEvenIfAskedLouder()   // start(f: 4000, levelDBFS: 0) → max |sample| <= 10^(-5/20) + 1e-6
@Test func levelMatchesRequest()            // levelDBFS -40 at 1000 → peak within 0.1 dB of 10^(-40/20)
@Test func onlyChosenEar()                  // ear .left → right buffer all zeros
@Test func rampsStartAndEndAtZero()         // first sample and the sample at 0.2 s are ~0; the envelope rises over 20 ms
@Test func threePulsesThenSilence()         // energy in [0,0.2), [0.4,0.6), [0.8,1.0); ~0 in the gaps and after 1.0 s; isFinished
@Test func catchTrialIsSilent()             // levelDBFS nil → all zeros, still lasts 1.0 s
```

- [ ] **Step 2: Run** `swift test --filter ToneSynthTests` — expect FAIL.
- [ ] **Step 3: Implement.** Phase-continuous sine and a raised-cosine envelope; the final per-sample `min(max(x, −capLin), capLin)` with `capLin = 10^(capDBFS(f)/20)` is applied after all gain, and stays even though the level is already clamped.
- [ ] **Step 4: Run** — expect PASS.
- [ ] **Step 5: Commit** `feat(core): pulsed test tone with hard level clamp`.

---

### Task 4: Measurement record, unique names, assignments and the output-switch decision

**Files:**
- Create: `Packages/AudioCurveCore/Sources/AudioCurveCore/OutputAssignments.swift`
- Create: `Packages/AudioCurveCore/Sources/AudioCurveCore/HearingTestRecord.swift`
- Modify: `Packages/AudioCurveCore/Sources/AudioCurveCore/ProfileStore.swift` (`SavedProfile`, new methods)
- Test: `Packages/AudioCurveCore/Tests/AudioCurveCoreTests/OutputAssignmentsTests.swift`, `ProfileStoreTests.swift`

**Interfaces:**
- Consumes: `TestCalibration`, `TestFlag`, `TestPoint` (Tasks 1–2).
- Produces:
  - `public struct HearingTestRecord: Codable, Equatable, Sendable { var date: Date; var output: OutputDevice; var calibration: TestCalibration; var left: [TestPoint]; var right: [TestPoint]; var flags: [TestFlag]; var appVersion: String }`
  - `SavedProfile.measurement: HearingTestRecord?` (decoded with `decodeIfPresent`; the memberwise init gets `measurement: HearingTestRecord? = nil`).
  - `ProfileStore.saveNew(id: UUID, name: String, profile: HearingProfile, measurement: HearingTestRecord?, uniqueName: Bool) throws(ProfileStoreError) -> SavedProfile` — with `uniqueName` a taken name gets " (2)", " (3)", …, truncating the base so the result is ≤ `maxNameLength`. The existing `saveNew(name:profile:)` calls it with `UUID()`, nil, false.
  - `public struct LevelSettings: Codable, Equatable, Sendable { var strength, inputGainDB, outputGainDB, tone, mpoDBFS: Double }`
  - `public struct OutputAssignment: Codable, Equatable, Sendable { var device: OutputDevice; var profileID: UUID?; var levels: LevelSettings? }`
  - `public struct OutputAssignments: Codable, Equatable, Sendable { var byUID: [String: OutputAssignment]; func assignment(_ uid: String) -> OutputAssignment?; mutating func assign(_ d: OutputDevice, profile: UUID?, levels: LevelSettings?); mutating func setLevels(_ l: LevelSettings, for uid: String); mutating func prune(existing: Set<UUID>) /* clears profileID of deleted profiles; drops entries with neither profile nor levels */ }`
  - `ProfileStore.loadAssignments() -> OutputAssignments` (missing/corrupt → empty) and `saveAssignments(_:)` (best effort), file `assignments.json`.
  - `public enum OutputSwitchAction: Equatable, Sendable { case overwrite(UUID, HearingProfile), saveUnsaved(HearingProfile), load(UUID), applyLevels(LevelSettings) }`
  - `public enum OutputSwitch { static func actions(assignment: OutputAssignment?, workingCopy: WorkingCopy?, savedSource: HearingProfile?, existing: Set<UUID>) -> [OutputSwitchAction] }`

**Switch rules (spec 5.2–5.3):** no assignment → `[]`. If the assigned profile exists and differs from `workingCopy?.sourceID`: first save edits — source exists and its saved profile ≠ working copy → `.overwrite(source, wc.profile)`; no source (or deleted) and `wc.profile != .flat()` → `.saveUnsaved(wc.profile)` — then `.load(id)`. A deleted or nil assigned profile → no profile actions. `levels` non-nil → `.applyLevels` last, even when the profile is unchanged.

- [ ] **Step 1: Write the failing tests**

```swift
// OutputAssignmentsTests
@Test func noAssignmentDoesNothing()          // actions(assignment: nil, …) == []
@Test func noWorkingCopyJustLoads()           // wc nil, assigned B exists → [.load(B)]
@Test func dirtySourceIsOverwrittenFirst()    // wc(source A, edited), saved A differs, assigned B → [.overwrite(A, edited), .load(B)]
@Test func cleanSourceIsNotOverwritten()      // saved A == wc → [.load(B)]
@Test func unsavedNonFlatIsSaved()            // wc(source nil, non-flat) → [.saveUnsaved(p), .load(B)]
@Test func flatUnsavedIsNotSaved()            // wc(source nil, .flat()) → [.load(B)]
@Test func sameProfileKeepsEdits()            // assigned A == source A, dirty → [] (plus .applyLevels if levels)
@Test func deletedAssignedProfileIgnored()    // B not in existing → only .applyLevels(l) when levels set, else []
@Test func levelsAppliedLast()                // [.load(B), .applyLevels(l)]
@Test func pruneClearsDeleted()               // prune(existing: []) on {uid: (B, levels)} keeps levels, profileID nil; {uid: (B, nil)} is dropped
// ProfileStoreTests
@Test func assignmentsRoundTrip()             // save then load equal; missing file → empty
@Test func measurementRoundTripsAndOldFilesDecode()  // saveNew with record → saved(id:)?.measurement == record; a file written without the key decodes with nil
@Test func uniqueNameAppendsSuffix()          // "AirPods Pro – 16 Nov 2026" twice → second is "AirPods Pro – 16 Nov 2026 (2)"
@Test func uniqueNameTruncatesBeforeSuffix()  // 70-char base taken → result.count <= 60 and ends with " (2)"
@Test func saveNewUsesGivenID()               // returned id == passed id
```

- [ ] **Step 2: Run** `swift test --filter "OutputAssignmentsTests|ProfileStoreTests"` — expect FAIL.
- [ ] **Step 3: Implement** the types and store methods.
- [ ] **Step 4: Run** `swift test` (whole package) — expect PASS.
- [ ] **Step 5: Commit** `feat(core): output assignments, measurement record, output switch rules`.

---

### Task 5: Channel commands, status and the testing route state

**Files:**
- Modify: `Packages/AudioCurveCore/Sources/AudioCurveCore/PluginChannel.swift`
- Modify: `Packages/AudioCurveCore/Sources/AudioCurveCore/RouteDecision.swift`
- Test: `PluginChannelTests.swift`, `RouteDecisionTests.swift`

**Interfaces:**
- Consumes: `OutputAssignments`, `HearingTestRecord` (Task 4).
- Produces:
  - `PluginCommand.assignProfile(OutputDevice, UUID?)` and `.saveMeasuredProfile(id: UUID, name: String, profile: HearingProfile, measurement: HearingTestRecord, assignTo: OutputDevice?)`
  - `PluginStatus.assignments: OutputAssignments` (init parameter with default `OutputAssignments()`)
  - `public enum AppIdentity { public static let bundleID = "se.lindefors.audiocurve" }` (in `PluginChannel.swift`; `AppModel` uses it instead of its literal fallback)
  - `RouteInputs.testing: Bool` (init parameter, default `false`)

- [ ] **Step 1: Write the failing tests**

```swift
@Test func newCommandsRoundTrip()     // both new cases encode via commandState and decode equal
@Test func statusCarriesAssignments() // encode/decode PluginStatus with one assignment → equal
@Test func testingPausesRoute()       // inputs(testing: true) → .paused(deviceUID: "airpods"); with output nil → .off
@Test func testingWinsOverPluginState() // testing with plugin .loading → .paused(…)
@Test func offStillWins()             // enabled false, testing true → .off
```

- [ ] **Step 2: Run** `swift test --filter "PluginChannelTests|RouteDecisionTests"` — expect FAIL.
- [ ] **Step 3: Implement.** In `routeTarget`, right after the `enabled` guard: `if inputs.testing { return inputs.outputUID.map { .paused(deviceUID: $0) } ?? .off }`; update its doc comment's order.
- [ ] **Step 4: Run** `swift test` — expect PASS.
- [ ] **Step 5: Commit** `feat(core): channel commands for assignments and measured profiles, testing route state`.

---

### Task 6: AU — profile and levels follow the output

**Files:**
- Modify: `AU/AudioCurveAudioUnit.swift`
- Modify: `AU/Parameters.swift` (helpers between `ParamValues` and `LevelSettings`)
- Modify: `docs/manual-test.md` (new section "Profile per output (2.2)")

**Interfaces:**
- Consumes: Task 4 and 5 types.
- Produces (on `AudioCurveAudioUnit`, main thread only):
  - `public private(set) var assignments: OutputAssignments` (loaded in `init` like `activeOutputs`, included in `channelStatus()` via the status snapshot)
  - `public func assign(_ d: OutputDevice, profile: UUID?)` — new assignments copy the current `LevelSettings`; saves; if `d.uid == currentOutputUID` and this is the app instance, runs the switch at once.
  - `public func saveMeasured(id:name:profile:measurement:assignTo:)` — `store.saveNew(…, uniqueName: true)`, then `assign` if `assignTo` is set.
  - `var isAppInstance: Bool { hostBundleID == AppIdentity.bundleID }`
  - `public var onAssignmentsChange: (@MainActor () -> Void)?`
  - `ParamValues.levels: LevelSettings` (get) and `mutating func apply(_ l: LevelSettings)`.

- [ ] **Step 1: Wire it.**
  - `run(_:)` handles the two new commands.
  - `outputDeviceChanged()`: if `isAppInstance`, reload assignments from disk, then execute `OutputSwitch.actions(assignment:workingCopy:savedSource:existing:)` in order: `.overwrite` → `store.overwrite`; `.saveUnsaved` → `store.saveNew(id: UUID(), name: "Unsaved – <date time>", …, uniqueName: true)` (name built with `Date.formatted(date: .abbreviated, time: .shortened)`; the prefix is a localized AU string); `.load` → `loadSaved(id:)`; `.applyLevels` → `setParameter` for the five addresses. Flush any pending working-copy autosave before computing the actions.
  - Level persistence: in the parameter observer, when the address is one of strength, inputGain, outputGain, tone, mpo and the current output has an assignment, schedule a debounced (0.5 s) `assignments.setLevels(…)` + save. **The work item captures `currentOutputUID` at scheduling time**; on output change, flush the pending item before switching.
  - Other hosts (not `isAppInstance`) never switch profiles or write levels.
- [ ] **Step 2: Build** `make release` — expect success; `cd dsp && cargo test` and `swift test` still pass.
- [ ] **Step 3: Write the manual test section** in `docs/manual-test.md`: assign profile A to speakers and B to AirPods → switching output loads each; edit in the editor then switch → edits are in A; levels differ per output and come back; a SoundSource instance does not switch; deleting B leaves AirPods on the current profile.
- [ ] **Step 4: Run the manual test** with the installed build (`make install`) and tick each line.
- [ ] **Step 5: Commit** `feat(au): profile and level settings follow the output`.

---

### Task 7: UI for assignments (menu panel and profile menu)

**Files:**
- Modify: `App/AppModel.swift` (`assignments`, `assign(_:profile:)`, mirror from `PluginStatus`)
- Modify: `App/MenuPanel.swift`
- Modify: `AU/UI/ProfileMenu.swift`, `AU/PluginViewModel.swift`
- Modify: `App/DemoSnapshot.swift` / `showDemoData()` (demo assignments so the panel screenshot shows the pop-ups)
- Modify: `App/Localizable.xcstrings`, `AU/Localizable.xcstrings`

**Interfaces:**
- Consumes: `PluginCommand.assignProfile`, `PluginStatus.assignments` (Task 5); `AudioCurveAudioUnit.assign`, `assignments`, `onAssignmentsChange` (Task 6).
- Produces: `AppModel.assignments: OutputAssignments`, `AppModel.assign(_ d: OutputDevice, profile: UUID?)`, `PluginViewModel.assignCurrentOutput()` and `isAssignedToCurrentOutput: Bool`.

- [ ] **Step 1: Menu panel.** Each "Active on" row gets a trailing `Picker` (menu style, labels hidden, fixed width ~110 pt): "—" (`UUID?.none`) plus each profile name. Panel width may grow to 340 if needed.
- [ ] **Step 2: Profile menu.** Add "Use on \(currentOutputName)" with a checkmark when assigned; disabled when there is no current output or the profile is unsaved (`sourceID == nil`).
- [ ] **Step 3: Strings** with all 7 translations.
- [ ] **Step 4: Build** `make release`; redraw `open build/release/AudioCurve.app --env AUDIOCURVE_DEMO=panel --env AUDIOCURVE_DEMO_OUT=docs/screenshots` and check the panel image in light and dark.
- [ ] **Step 5: Commit** `feat(app): choose a profile per output in the menu panel and profile menu`.

---

### Task 8: Tone generator and device volume

**Files:**
- Create: `App/Audio/ToneGenerator.swift`
- Create: `App/Audio/DeviceVolume.swift`

**Interfaces:**
- Consumes: `ToneSynth` (Task 3), `HAL` (`App/Audio/CoreAudioSupport.swift`).
- Produces:
  - `final class ToneGenerator` — `init(deviceUID: String) throws` (AVAudioEngine; output unit bound to the device via `kAudioOutputUnitProperty_CurrentDevice`; an `AVAudioSourceNode` whose render block calls `ToneSynth.render`); `func start() throws` (engine runs silent); `func play(ear: Ear, frequencyHz: Double, levelDBFS: Double?)`; `var isPlaying: Bool`; `func stop()`. Tone requests reach the render block through a lock-free handoff (`Synchronization.Atomic` or a single-slot `Mutex` try-lock); the render block never allocates.
  - `struct DeviceVolume` — `static func read(uid: String) -> VolumeSnapshot?` (per-element `kAudioDevicePropertyVolumeScalar`, output scope, elements 0, 1, 2 where settable); `static func setMax(uid: String) -> Bool` (false if not settable); `static func restore(_ s: VolumeSnapshot)`.
  - `struct VolumeSnapshot: Codable { var deviceUID: String; var scalars: [UInt32: Float32] }` with `static func savePending(_:)`, `static func pending() -> VolumeSnapshot?`, `static func clearPending()` (UserDefaults key `hearingTest.pendingVolume`).

- [ ] **Step 1: Implement both files.** Order in callers (Task 9): `start()` (silent) → `savePending(read)` → `setMax` → read back and check ≥ 0.99 → first `play`.
- [ ] **Step 2: Build** `make release` — expect success.
- [ ] **Step 3: Commit** `feat(app): tone generator and device volume save/restore`.

---

### Task 9: Hearing test model, view and app integration

**Files:**
- Create: `App/HearingTestModel.swift`
- Create: `App/HearingTestView.swift`
- Modify: `App/AppModel.swift` (`testing` flag into `RouteInputs`; restore a pending volume in `start()` when the device is present; `saveMeasured(…)`; status line text)
- Modify: `App/HostView.swift` ("Hearing test…" button; swaps the plugin view for `HearingTestView`)
- Modify: `App/Localizable.xcstrings`
- Modify: `docs/manual-test.md` (section "Hearing test (2.2)")

**Interfaces:**
- Consumes: `HearingTest`, `HearingTestResult`, `TestCalibration`, `HearingTestRecord` (Tasks 1–4); `ToneGenerator`, `DeviceVolume`, `VolumeSnapshot` (Task 8); `PluginCommand.saveMeasuredProfile`, `PluginStatus.profiles` (Task 5).
- Produces:
  - `@Observable @MainActor final class HearingTestModel` with `enum Phase { case intro, running(Presentation), paused(PauseReason), done(HearingTestResult), saving(UUID), saveFailed }`, `enum PauseReason { case user, otherSound }`, and `start()`, `respond()`, `pause()`, `resume()`, `cancel()`, `save(name: String, assign: Bool)`.
  - `AppModel.testing: Bool` and `AppModel.saveMeasured(id:name:profile:measurement:assignTo:)` (sends the command).

- [ ] **Step 1: Model.** Timing per presentation: wait `gapBeforeS`, play (`ToneSynth.duration`), then a 1.5 s response window; no press → `respond(heard: false)`. Pressing outside a window does nothing to the staircase. Stop the test (tones off, volume restored, phase `intro` with a message) when: the current output changes, `otherHosts` becomes non-empty, or the window closes. `HAL.othersPlaying()` polled every second → `paused(.otherSound)`. A pause longer than 2 min restores the volume and raises it again on resume. Volume is restored on finish, cancel and `applicationWillTerminate`; then `VolumeSnapshot.clearPending()`.
- [ ] **Step 2: Save.** `save` generates the `UUID`, builds the profile from `result.audiogram(.left/.right)` with zero trims, sends `saveMeasured`, and polls `AppModel.profiles` every 0.25 s for that id; found → leave the test view; not found after 3 s → `saveFailed` with Retry (resends the same id), result kept.
- [ ] **Step 3: View** per spec 4.2: intro (checklist incl. "turn off any system-wide EQ or sound enhancer", transducer picker "In-ear / Over-ear" defaulting to `TestCalibration.defaultTransducer`, warnings for another app playing, for `otherHosts` non-empty (Start disabled, names the app via `AppModel.appName`), and for a device without settable volume), running (ear, frequency, progress `index / 24`, big "I hear it" button, Space via `.onKeyPress(.space)` ignoring `isARepeat`, Pause, Cancel), done (audiogram drawn in the editor's style, flags as warnings with the spec's texts, including asymmetry: "The worse ear may be underestimated. An audiologist can measure it with masking.", the label "Relative to <device>", the suggestion to press Auto level, name field defaulting to "<device> – <date>", "Use on <device>" checkbox on, Save, Discard).
- [ ] **Step 4: Strings** with all 7 translations.
- [ ] **Step 5: Build** `make release`; `swift test`.
- [ ] **Step 6: Write and run the manual test section:** a full test on AirPods; a tone never sounds unexpectedly loud (check with the volume at max before starting); force-quit mid-test → volume restored on next launch; switch output mid-test → test stops, volume restored; start SoundSource with AudioCurve → Start disabled; play music in another app → test pauses; the saved profile appears, is assigned and active.
- [ ] **Step 7: Commit** `feat(app): hearing test`.

---

### Task 10: Docs and version

**Files:**
- Modify: `README.md` (Features: "Hearing test" and "Profile per output" bullets; "Using it": a test step in the typical setup; the "Not a medical device" note mentions the test is not a clinical audiogram)
- Modify: `project.yml` (`MARKETING_VERSION: "2.2.0"`, bump the build number as in commit 2fc5cf9), AU component version `0x00020200` where 2fc5cf9 changed it

- [ ] **Step 1: Edit** the files.
- [ ] **Step 2: Verify** `xcodegen generate && make release && (cd Packages/AudioCurveCore && swift test) && (cd dsp && cargo test)` — all pass.
- [ ] **Step 3: Commit** `docs: AudioCurve 2.2 hearing test and profile per output`.
