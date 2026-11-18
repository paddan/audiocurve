# AudioCurve 2.2: hearing test per headphone, profile per output

Date: 2026-11-16
Status: draft for review

## 1. Intent

Today a user must already have an audiogram (from an audiologist) and type it into the editor. AudioCurve 2.2 lets the user measure their own audiogram **through the headphones they listen with**. The result describes ear + headphone together, which is exactly what AudioCurve compensates: the headphone's own response is baked into the curve.

Because a measured audiogram only makes sense on the headphone it was measured on, profiles also learn to **follow the output device**: each output can have a profile assigned, and AudioCurve switches to it when that output becomes the system output.

The user's own words: the audiogram becomes specific to those headphones; with AirPods it can be something close to a real audiogram. Research for this design found no published per-volume-step SPL calibration for AirPods, and AirPods Pro 3 change their EQ with volume, so in 2.2 **all headphones are treated alike**: the result is "relative to these headphones". A calibration table per model is a later step (section 7), and the data model leaves room for it.

## 2. Goals, non-goals, anti-goals

Goals:

- A thorough hearing test, about 10–15 minutes, both ears, all 11 standard frequencies (125 Hz – 8 kHz), using a clinical-style method (section 4.3).
- The result is saved as a normal named profile and assigned to the output it was measured on.
- Any saved profile can be assigned to any output; switching the system output switches the profile.
- The test is safe: no tone can ever play louder than a fixed maximum, even if something fails.

Non-goals (2.2):

- Calibrated dB HL comparable with an audiologist (section 7).
- A quick test mode. Can be added later on the same engine.
- Masking for asymmetric losses (see section 4.6 for the warning instead).
- Running the test from the AU in other host apps: the host owns volume and output, so the headphone is not known.
- Measuring background noise with the microphone.

Anti-goals (failure even if it technically works):

- A tone that is unexpectedly loud. A user with hearing loss must never get a full-scale tone at max hardware volume because a gain stage was skipped.
- The system volume left at max after the test (crash, quit, output change).
- A result that looks precise but is not: unreliable runs must be flagged, not saved silently as fact.
- Losing the user's unsaved edits when the output, and thus the profile, changes.

## 3. Approaches

Considered:

1. **Test in the app window, tone generator in Swift (chosen).** A new "Hearing test" mode in the main window. AVAudioEngine plays tones straight to the current output; AudioCurve's processing is paused during the test. Test logic (staircase, thresholds, conversion) lives in `AudioCurveCore` and is unit-tested without audio.
2. **Tone generator in the Rust kernel through the normal render path.** Same path and ceiling as normal listening, but needs a test mode through FFI, the process tap and the plugin, with compressors and limiter switched off mid-kernel. Much more work for the same result; the safety cap can be a hard clamp in the Swift generator.
3. **Test inside the AU's UI, usable in every host.** The host controls volume and output, so the headphone is unknown — defeats the purpose. Rejected.

The user picked 1.

Decided with the user:

- Profiles are tied to outputs (option B), and **any** profile can be assigned to an output, not only a measured one.
- Level settings (Strength, Input/Output level, Tone, Ceiling) follow the output too, stored in the assignment.
- The tone level cap is per frequency, up to about 95 dB SPL nominal (section 4.5).
- Thorough test, all 11 frequencies.
- No calibration in 2.2 (option A); room in the data model for it later.

## 4. The hearing test

### 4.1 Where it lives

- **`AudioCurveCore` (new, pure, tested):** `HearingTest` — the procedure as a state machine. Input: responses (heard / not heard / timeout). Output: the next tone to play (ear, frequency, level in dBFS, timing) or a finished result. No audio, no clock: time is injected so tests run instantly.
- **`AudioCurveCore` (new):** `TestCalibration` — conversion between tone level (dBFS at max hardware volume) and dB HL (section 4.4).
- **App (new):** `ToneGenerator` (AVAudioEngine output to a given device), `HearingTestModel` (drives `HearingTest`, owns volume save/restore), `HearingTestView`.
- **App (changed):** `AppModel` pauses the route during the test; `HostView` gets a "Hearing test…" button that swaps the plugin view for the test view.

### 4.2 Flow the user sees

```
[Hearing test]                                         Output: AirPods Pro
 1. Before you start
    • Sit somewhere quiet. Turn on noise cancellation if your headphones have it.
    • Stop other sound on the Mac.             (warns if another app is playing)
    • The test sets the headphone volume to max and controls the level itself.
      The volume is restored afterwards.
    [Start test]   [Cancel]

 2. Testing — Right ear, 2000 Hz                         ███████░░░░  9 / 22
    Press Space or click when you hear a tone.
                   [ I hear it ]
    [Pause]  [Cancel]

 3. Done
    (audiogram, left and right, in the editor's style)
    ⚠ The 1 kHz retest differed by 15 dB in the left ear — consider testing again.
    Name: [AirPods Pro – 16 Nov 2026]   ☑ Use on AirPods Pro
    [Save]   [Discard]
```

- Order per ear (clinical): 1000, 1500, 2000, 3000, 4000, 6000, 8000, 1000 (retest), 750, 500, 250, 125 Hz. Right ear first, then left.
- Tone: pulsed pure tone, 3 pulses of 200 ms on / 200 ms off (≈1 s), 20 ms raised-cosine ramps. Pulsed tones are easier to detect for people with tinnitus.
- Response window: 1.5 s after the tone ends. A press outside a tone window counts as a false press.
- Random gap between presentations: 1.5–3.5 s, so the rhythm cannot be anticipated.
- Space bar and the on-screen button both answer. Pause stops tones until resumed.

### 4.3 Procedure (modified Hughson–Westlake)

Per ear and frequency:

1. Start at a familiar level (nominal 40 dB HL). If not heard, raise 20 dB until heard (up to the maximum).
2. After each "heard": down 10 dB. After each "not heard": up 5 dB.
3. Threshold = the lowest level heard on **2 of 3 ascending** presentations (or 2 of 2 if only two ascents were possible).
4. Cap: if not heard at the maximum level (section 4.5), the point is recorded as "no response at max" and stored as that maximum, flagged.
5. Bail-out: if a frequency takes more than 12 presentations without converging, take the best estimate so far and flag it.

Reliability checks:

- **Retest:** 1 kHz is measured twice per ear; a difference of more than 10 dB flags the result.
- **Catch trials:** about 1 in 8 presentations is silent. More than 2 presses on catch trials in a run flags the result ("you may be pressing without hearing a tone").
- Flags are shown on the result screen and stored with the profile (section 6.1). They never block saving.

### 4.4 Conversion to dB HL

There is no calibration in 2.2, so the conversion uses a **nominal assumption**, stored with every result:

```
HL(f) = threshold_dBFS(f) + nominalFullScaleSPL − nominalRETSPL(f)
```

- `nominalFullScaleSPL`: SPL of a 0 dBFS sine at max hardware volume. Default 100 dB SPL (typical headphones, ~95–105).
- `nominalRETSPL(f)`: reference equivalent threshold SPL per frequency. Two tables, because in-ear and over-ear differ by up to ~15–20 dB at low frequencies and that changes the curve's shape: **in-ear** (insert earphone values, ISO 389-2 ballpark) and **over-ear/on-ear** (circumaural, ISO 389-8 ballpark). The start screen asks "In-ear or over-ear?" (defaulting to in-ear when the device name contains "AirPods"), and the choice is stored in `TestCalibration`. Exact values and source are the builder's call, documented in code. (My call.)
- Result clamped to the profile range −10…120 dB HL and rounded to 5 dB like a clinical audiogram.

Because the fitting rule turns dB HL into gain directly, an error in `nominalFullScaleSPL` shifts all gains equally, which the user already corrects with Strength and Input/Output level. The shape of the curve — which is what the test measures — is not affected. The result is labelled "relative to <headphone>" in the UI.

### 4.5 Safety

- **Hard level cap per frequency in the generator (decided with the user):** a fixed cap of about 95 dB SPL nominal at 1–8 kHz, so moderate-to-severe losses can still be measured. The cap is a constant table in `ToneGenerator`, not something the UI can raise:

  | Frequency | Cap |
  |---|---|
  | 125–500 Hz | −1 dBFS |
  | 750–1500 Hz | −3 dBFS |
  | 2000–8000 Hz | −5 dBFS |

  The low frequencies get more headroom because the ear's threshold is higher there and the nominal HL range under a flat cap would be too small (≈45 dB HL at 125 Hz). With the nominal assumption this gives a measurable top of roughly 60–65 dB HL at 125 Hz and 85–90 dB HL at 4–8 kHz. Every sample is clamped to the cap for the tone's frequency after all gain stages; the staircase only reaches it for someone who does not hear the levels below it. The `HearingTest` state machine also never asks for more than the cap (tested); the clamp in the generator is the second guard.
- **Start quiet:** the first tone of each ear is preceded by nothing louder than 40 dB HL nominal; the 20 dB jumps stop at the cap. Above 80 dB HL nominal the steps up are 5 dB only (no 20 dB jumps).
- **Volume order:** the generator's digital gain is set to silence *before* the hardware volume is raised, and the generator verifies it is running at the expected attenuation before the first audible tone.
- **Volume save/restore:** before raising the volume, the original device volume (per channel, scalar) is written to `UserDefaults` together with the device UID. It is restored on finish, cancel, every pause (the volume is raised only while the test runs; Resume re-checks other sound and mute, as Start does, then raises it again), output change, app quit, and — if the app crashed — on the next launch when that device is present.
- **Output change mid-test:** the test stops at once (tones off, volume restored on the old device if still present), and the user can restart.
- If the device has no settable volume (some USB DACs, HDMI), the test still runs, but the start screen says the level may be off and asks the user to set the volume to max themselves.

### 4.6 Asymmetry warning

With headphones, a loud tone in the worse ear can be heard by the better ear (cross-hearing, ~40–60 dB interaural attenuation). Without masking (non-goal), the result screen warns when the two ears differ by 40 dB or more at any frequency: "The worse ear may be underestimated. An audiologist can measure it with masking."

### 4.7 Pausing AudioCurve during the test

- `RouteInputs` gets a `testing: Bool`. While true, `routeTarget` returns `.paused` (existing state), so the tap and the plugin stop processing and the test tone is never captured or processed.
- **Other AudioCurve instances:** if `PluginStatus.otherHosts` is not empty (AudioCurve running in SoundSource, a DAW, …), the test refuses to start and says which app to quit, because that instance sits after the generator's clamp and could amplify the tone. If one appears during the test, the test stops. Other system-wide processors cannot be detected; the start screen asks the user to turn off any system EQ. (My call.)
- `HAL.othersPlaying()` (exists) is used on the start screen to warn that another app is making sound, and polled during the test: if it turns true, the test pauses and asks the user to stop the other sound.
- The status line shows "Hearing test in progress".

## 5. Profile per output

### 5.1 Model

- New `OutputAssignments` in `AudioCurveCore`: a map from output UID to `OutputAssignment` (device, profile, levels; section 5.2). Stored by `ProfileStore` as `assignments.json`, next to `outputs.json`.
- Assignment lives **in the plugin**, like `ActiveOutputs`, because the plugin owns the working copy and already monitors the system output. The app changes it through new `PluginCommand` cases:
  - `.assignProfile(OutputDevice, UUID?)` — `nil` removes the assignment.
  - `.saveMeasuredProfile(name: String, profile: HearingProfile, measurement: HearingTestRecord, assignTo: OutputDevice?)` — the AU is sandboxed, so the app cannot write its profile directory itself. The app generates the profile's `UUID` and puts it in the command. If the name is taken, the plugin appends " (2)", " (3)", …. The app confirms the save by polling `PluginStatus` for that `UUID`; until it appears (timeout 3 s) the result screen keeps the measurement and shows "Saving…", and on timeout it shows an error with Retry, never discarding the result. (My call.)
- `PluginStatus` gets `assignments: OutputAssignments`.

### 5.2 Level settings follow the output (decided with the user)

Because the test is not calibrated, each headphone needs its own levels. An assignment therefore also carries the level settings:

```swift
struct OutputAssignment: Codable { var device: OutputDevice; var profileID: UUID?; var levels: LevelSettings? }
struct LevelSettings: Codable { var strength, inputGainDB, outputGainDB, tone, mpoDBFS: Double }
```

- While the current output has an assignment, changes to these five AU parameters are saved into its `levels` (debounced, like the working copy).
- On an output change, the new output's `levels` (if any) are applied to the parameters together with its profile. An output without an assignment keeps the current levels, as today.
- A new assignment copies the current levels. After a hearing test the result screen suggests pressing **Auto level** once.
- Mode, the lab switches and Bypass are not part of `LevelSettings`.
- The profile file format is unchanged.

### 5.3 Behaviour

- **Only the app's own plugin instance switches profiles** (the instance whose host identified itself with the app's bundle ID via `.identifyHost`). Other hosts (SoundSource, DAWs) keep the profile they have, as today, so instances never overwrite each other's profiles or create duplicate "Unsaved" profiles, and a DAW project keeps its chosen profile. (My call; replaces an earlier idea that every host would follow.)
- When the system output changes, the app's instance looks up the new output. If it has an assigned profile and that profile exists, it is loaded. If not, the current profile stays — same as today.
- An assignment to a deleted profile is ignored (treated as none) and removed the next time assignments are saved.
- **Unsaved edits:** if the working copy has changes that are not in its source profile when the output switches, they are first saved into that source profile (autosave), then the new profile is loaded. A working copy with no source (never saved) that differs from a flat profile is saved as a new profile named "Unsaved – <date time>" before switching, so nothing is lost. (My call.)
- Loading a profile by hand while on an output **does not** change the assignment. Assigning is explicit (section 5.4).
- **Assigning to the current output loads the profile at once**, through the same path as an output change (unsaved-edits rule first). So after Save on the test's result screen the new profile is active immediately. (My call.)

### 5.4 UI

- **Menu panel:** each row under "Active on" gets a small profile pop-up on the right: "—" (none) or a saved profile. The top "Profile" picker still loads a profile by hand.
- **Window profile menu (`ProfileMenu`):** a "Use on <current output>" item that assigns the loaded profile to the current output, with a checkmark when it already is.
- The test's result screen assigns to the measured output by default (checkbox, on).

## 6. Data

### 6.1 Measurement record

`SavedProfile` gets an optional `measurement: HearingTestRecord?` (optional, so existing files still decode; `fileVersion` stays 1):

```swift
struct HearingTestRecord: Codable {
    var date: Date
    var output: OutputDevice              // what it was measured on
    var calibration: TestCalibration       // the nominal assumption used (4.4); later a real table
    var thresholdsDBFS: [Ear: [Double: Double]]   // raw, before conversion, so it can be re-converted
    var flags: [TestFlag]                  // retestMismatch(ear, dB), catchTrialPresses(n), noResponseAtMax(ear, f), notConverged(ear, f), asymmetry
    var appVersion: String
}
```

Storing the raw dBFS thresholds means a later calibration table (section 7) can re-convert old measurements without a new test.

### 6.2 Profile content

The audiogram is the converted result for each ear. Trims are zero. The profile is otherwise a normal profile: editable, renamable, assignable.

## 7. Later (not in 2.2)

- **Calibration per headphone model** (biological calibration with a normal-hearing listener, or coupler measurement), shipped as a table keyed by model and applied through `TestCalibration`. Old measurements are re-converted from their raw dBFS thresholds.
- A quick test mode (6 frequencies) on the same `HearingTest` engine.
- Masking for asymmetric losses.

## 8. Left to the builder

- The exact RETSPL table and its source, and the `nominalFullScaleSPL` default (≈100 dB SPL).
- Tone synthesis details (sample rate follows the device; phase-continuous, ramps as above).
- How `ToneGenerator` sets and verifies device volume (Core Audio `kAudioDevicePropertyVolumeScalar` per channel or master).
- Layout details of the test view; it should match the existing window's style and work in light and dark mode.
- Strings in English in the existing `Localizable.xcstrings`, like the rest of the app.
- Tests: `HearingTest` (staircase, 2-of-3 rule, cap never exceeded, catch trials, retest flag, bail-out), `TestCalibration` (conversion, clamp, rounding), `OutputAssignments` and plugin output switching (including deleted profiles, unsaved edits, levels saved and restored per output, and that only the app's instance switches), the per-frequency cap table never exceeded, `PluginChannel` round-trips for the new commands, and `routeTarget` with `testing`. A manual test section in `docs/manual-test.md` for the audio side (volume restore after force-quit, output change mid-test, the level cap on a real device).
