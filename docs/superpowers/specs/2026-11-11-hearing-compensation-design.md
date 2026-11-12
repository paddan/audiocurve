# audiocurve: hearing compensation as an AUv3 plugin (v1 design)

Date: 2026-11-11

Status: the original v1 design. The project has grown since: speech/music auto mode, input level, spectrum view, saved profiles, a safety meter and localization. See the README for what the plugin does today.

## 1. Purpose

Adapt the Mac's audio to a listener's audiogram, roughly like iPhone's *Headphone Accommodations* with AirPods. Hearing losses above 100 dB HL in some bands must be handled.

The signal processing follows the principles of modern hearing aids: multi-channel compression (WDRC) and output limiting (MPO), not a static EQ.

v1 is a **proof of concept and of sound quality**, built as an Audio Unit v3 effect. It runs in **SoundSource** (Rogue Amoeba), which handles capture, routing and the choice of output or app.

**Scope of use:** a single developer's own Mac (arm64), with no distribution.

**A successful v1 means:**
- The plugin loads in SoundSource, both system-wide and per app, and in the bundled host app.
- Audio is audibly compensated according to the audiogram, without crackles or dropouts.
- Profile and settings survive a restart of SoundSource.
- The plugin adds at most 5 ms of latency (the limiter's 2 ms lookahead plus margin).

## 2. Scope

**In v1:**
- A stereo AUv3 effect with the parameters strength and MPO.
- One audiogram per ear, entered by hand in the plugin's UI and stored in the plugin's state.
- A multiband compressor (WDRC) per ear with an open prescription rule and a trim per band.
- An MPO limiter.
- Output gain before the limiter, with "Auto level" matching the output level to the input level.
- Input and output meters.
- A Speech/Music mode switch (time constants).
- A host app that plays an audio file through the plugin, with A/B switching.

Three of these ideas (output gain with auto level, meters, and Speech/Music) come from a review of EarFix (MIT). No code was copied. EarFix's 40 dB per-band gain cap and its maximum compression ratio of 3:1 are deliberately **not** adopted.

**Not in v1:**
- Health import (file or HealthKit).
- A custom capture layer (process tap); SoundSource chooses the output.
- Frequency lowering, planned for v2. The kernel must allow it as a separate stage before the filterbank.
- Moving high frequencies from the worse to the better ear (CROS-like), an idea for v2.
- Headphone correction via AutoEq profiles, an idea for v2.
- Mono and multichannel.
- Distribution and notarization.
- Licensed prescription rules (NAL-NL2, DSL v5).

## 3. Technical basis

- macOS 27, Xcode 27.2, Swift 6, Rust stable (1.99), target `aarch64-apple-darwin`.
- The Xcode project is generated with XcodeGen from `project.yml`.
- Split between languages:
  - **Rust** (`dsp/`, crate `audiocurve-dsp`): all realtime processing, exposed through a C ABI.
  - **Swift** (`Packages/AudioCurveCore`): profile model, validation, prescription rule and state serialization.
  - **Swift** (app and extension): the AUv3 shell, the UI and the host app.
- AUv3 identity: type `aufx`, subtype `hcmp`, manufacturer `Lnds`, name `audiocurve: Hearing Compensation` (renamed `AudioCurve: AudioCurve` after v1.0.0).
- Signing: Xcode's automatic signing with a personal team if available, otherwise "Sign to Run Locally". An AUv3 extension always runs in a sandbox.

## 4. Components

### 4.1 `audiocurve-dsp` (Rust)

- `crate-type = ["staticlib", "rlib"]`, with no runtime dependencies. `assert_no_alloc` may be used as a dev dependency.
- **C ABI** (header `dsp/include/audiocurve_dsp.h`):
  ```c
  typedef struct { double gain_soft_db, gain_loud_db, compression_ratio; } ac_band_fitting;
  typedef struct {
      ac_band_fitting left[12], right[12];
      double mpo_dbfs;        // −20…−1
      double output_gain_db;  // −48…+12, applied before the limiter
      double attack_ms, release_ms; // Speech 5/50, Music 10/200
  } ac_params;
  typedef struct { float in_rms_db[2], in_peak_db[2], out_rms_db[2], out_peak_db[2]; } ac_meters;
  typedef struct ac_kernel ac_kernel;
  ac_kernel* ac_kernel_new(double sample_rate, uint32_t max_frames); // NULL on invalid arguments
  void ac_kernel_free(ac_kernel*);
  void ac_kernel_set_params(ac_kernel*, const ac_params*); // single writer, any thread, lock-free
  void ac_kernel_process(ac_kernel*, float* left, float* right, uint32_t frames); // realtime, in-place
  uint32_t ac_kernel_latency_frames(const ac_kernel*);
  void ac_kernel_reset(ac_kernel*);
  void ac_kernel_read_meters(const ac_kernel*, ac_meters* out); // any thread, reads atomics
  ```
- **Meters:** RMS with 300 ms integration and peak with a 1.5 s fall, in dBFS (floor −120).
  - The measuring points are *in* (after sanitizing) and *out* (after the limiter).
  - Values are written as atomic `f32` bits once per block.
- **Chain in `process`:**
  1. Non-finite samples are replaced with 0.
  2. Pre-processing stage (identity, reserved for v2).
  3. Per ear: filterbank → compressor per band → sum.
  4. Output gain, smoothed the same way as the band gains.
  5. Linked stereo limiter.
  6. Metering.
  7. Blocks larger than `max_frames` are split internally.
- **Filterbank:**
  - 12 bands with centre frequencies 125, 250, 500, 750, 1k, 1.5k, 2k, 3k, 4k, 6k, 8k and 12 kHz.
  - LR4 crossovers (two Butterworth biquads, q = 1/√2) at the geometric mean between adjacent centre frequencies, in a tree with allpass compensation.
  - The bands sum flat within ±0.5 dB.
- **Band compressor:**
  - Power envelope with attack and release from `ac_params`: Speech mode is 5/50 ms and Music mode 10/200 ms.
  - The level is computed as `10·log10(2e + 1e-20)` dBFS, with a full-scale sine at 0 dBFS. The static curve is in 4.2.
  - The applied gain is smoothed (τ = 20/3 ms), so parameter changes fade in over about 20 ms.
- **MPO limiter:** 2 ms lookahead, peak-based, linked stereo, 0.5 ms attack and 50 ms release. The ceiling is `min(mpo_dbfs, −1)`, after which the signal is hard-clamped to the ceiling.
- **Realtime rules:** no allocations, locks, syscalls or logging in `process`. Parameters are handed over through a triple buffer with `AtomicU8`.

### 4.2 `AudioCurveCore` (Swift)

- `Audiogram`, `AudiogramPoint`, `Ear`, `HearingProfile`.
  - The profile holds an audiogram per ear, a trim per band and ear (−20…+20 dB, 12 values) and `validate()`.
  - Rules: at least 4 points per ear; unique, sorted frequencies among 125, 250, 500, 750, 1000, 1500, 2000, 3000, 4000, 6000 and 8000 Hz; values between −10 and 120 dB HL.
- `FittingRule`:
  - `interpolatedHL`: linear in log2(f), constant outside the measured points.
  - `gainDB(band, inputLevelDBFS)`: the static curve.
  - `parameters(profile:strength:mpoDBFS:) -> ac_params`.
- **The rule (v1, open):** per band, `HL = max(0, interpolated)`. The low-frequency correction `corr` is 10 dB at 125 and 250 Hz, 5 dB at 500 Hz, and 0 otherwise.
  - `G_soft = (max(0, 0.6·HL − corr) + trim)·s` at an input of −60 dBFS.
  - `G_loud = (max(0, 0.25·HL − corr) + trim)·s` at an input of −15 dBFS.
  - `CR = clamp(45 / (45 − (G_soft − G_loud)), 1, 10)`.
  - Below −75 dBFS: 1:2 expansion. Between −75 and −60 dBFS: constant `G_soft`. Above −60 dBFS: `G_soft + (L + 60)·(1/CR − 1)`.
  - Example: HL 100 and s = 1 give 60 dB, 25 dB and CR 4.5.
  - There is no flat gain cap; the output level is limited by the MPO and the hardware.
- `PluginState`: `Codable` with `version: Int = 1` and `profile: HearingProfile?`. The JSON lives under the key `"audiocurve.state"` in the AU's `fullState`.

### 4.3 `AudioCurveAU` (AUv3 app extension, Swift)

- An `AUAudioUnit` subclass with one stereo, non-interleaved Float32 input bus and output bus. Other formats are rejected in `shouldChangeToFormat`, or with an error in `allocateRenderResources`.
- **Parameters** (`AUParameterTree`):
  - `strength`: 0–100 %, default 50
  - `mpo`: −20…−1 dBFS, default −3
  - `outputGain`: −48…+12 dB, default 0
  - `mode`: indexed, Speech (0) or Music (1), default Speech

  All of them can be automated and are saved in host presets.
- **Auto level** (UI and Swift, not realtime):
  - While audio plays, the UI reads the meters every 50 ms for 3 s.
  - It computes the mean RMS (power mean over both channels) for in and out and sets `outputGain += in − out`, clamped to the parameter range.
  - If the input is below −50 dBFS, the measurement stops with the message "Too quiet – play something".
- **`fullState`:** get appends `PluginState` to the base class state, and set reads and validates it. If the data is missing or the profile is invalid, `profile = nil`.
- **Profile or parameter change:** `FittingRule.parameters(...)` and then `ac_kernel_set_params` run off the audio thread. With `profile = nil`, normal hearing is used, which means transparent audio.
- **`allocateRenderResources`:** `ac_kernel_new(sampleRate, maximumFramesToRender)`. `deallocateRenderResources` calls `ac_kernel_free`.
- **`internalRenderBlock`:** the block captures only a raw kernel pointer. It pulls input via `pullInputBlock` into the output buffer and then calls `ac_kernel_process` in place. It does no Swift object work.
- `latency` = `ac_kernel_latency_frames / sampleRate`, and `tailTime` = 0.1 s.
- **UI** (`AUViewController` with SwiftUI):
  - an audiogram graph edited in place (log x, y −10…120 dB HL inverted, left ear a blue cross, right ear a red ring)
  - sliders for strength, MPO and output gain, plus an "Auto level" button
  - the Speech/Music switch
  - input and output meters
  - a trim per band and ear
  - a gain graph per band for soft and loud input
  - inline validation
  - a "No profile" state when no profile exists

  The visual design (layout, typography, colours) is set in section 4.4.

### 4.4 UI design (layout A: dashboard)

- **Window:** preferred size 720×420 pt, at least 640×380, resizable.
  - It follows the system's light and dark mode.
  - Ear colours follow the audiogram convention: left blue (#378ADD) and right red (#E24B4A). Everything else uses system colours and the SF font.
- **Left area, about 60 %:**
  - an **Audiogram / Gain** switch and the graph area
  - below the graph, a legend "✕ Left ○ Right", where a click picks the *active* ear
- **Right column:**
  - the Speech/Music switch
  - the sliders Strength (%), Output level (dB) and Ceiling/MPO (dBFS), with the current value on the right
  - the "Auto level" button. During the measurement it shows "Measuring… 3 s", and then the result or "Too quiet – play something".
  - the In and Out meters (RMS bar and peak marker, −60…0 dBFS)
- **Without a profile:** the graph area shows "No audiogram" and an "Add audiogram" button. The button creates a profile with all 11 frequencies at 0 dB HL for both ears.
- **Editing the audiogram** (spec requirement):
  - **Clicking a point** selects it, with a focus ring and the label "L 1.5 kHz · 55 dB HL". Where points from both ears overlap, the active ear wins.
  - **Click and drag** vertically changes the value directly in 5 dB steps, or 1 dB with ⌥ held, clamped to −10…120. Clicking an empty standard frequency adds a point there for the active ear.
  - **Keyboard**, with a point selected:
    - ↑/↓ change ±5 dB (±1 with ⌥). ↑ means up in the graph, so a lower dB HL.
    - ←/→ go to the previous or next point on the same ear.
    - Tab switches ear (the same frequency if it exists, otherwise the nearest).
    - Digits followed by Enter set the value directly.
    - ⌫ deletes the point. This is refused if fewer than 4 points would remain, and the message "At least 4 points per ear" is shown.
    - Esc deselects.
  - Changes apply to the audio immediately.
  - VoiceOver: every point is adjustable (increment and decrement give ±5 dB).
- **Gain view:**
  - Bars per band and ear show the gain for soft input, and a dashed mark shows the gain for loud input.
  - Trim is changed with the same interaction as the audiogram: drag or ↑/↓ in 1 dB steps, clamped to ±20 dB.
  - ←/→ switch band and Tab switches ear. A double-click resets the trim.

### 4.5 Host app `audiocurve` (Swift)

- A window that instantiates the plugin via `AVAudioUnit.instantiate` and shows its UI.
- It loops a chosen audio file through `AVAudioEngine`: player → plugin → output.
- It has an **A/B switch** (bypass) and a level meter (peak, dBFS) after the plugin.

## 5. Data flow

- **Audio:** host → `pullInputBlock` → `ac_kernel_process` → host.
- **Control:** UI or automation → `PluginState`/parameters → `FittingRule` → `ac_params` → `ac_kernel_set_params` → triple buffer → the kernel fades in the new values.

## 6. Error handling and safety

| Situation | Behaviour |
|---|---|
| Invalid profile in the UI | Shown inline and never sent to the kernel |
| `fullState` missing, corrupt or of unknown version | `profile = nil` and transparent audio |
| Unsupported format (mono, multichannel) | Rejected; the host shows that the plugin cannot be used |
| `ac_kernel_new` returns NULL | `allocateRenderResources` throws `kAudioUnitErr_FailedInitialization` |
| NaN or Inf in the input | Zeroed, and the kernel recovers immediately |
| Changed sample rate | The host calls deallocate and allocate, and the kernel is recreated |

The limiter is always active and has an absolute ceiling of −1 dBFS. The host's bypass works as usual.

## 7. Testing

**`cargo test`** (in `dsp/`):
- the filterbank sums flat within ±0.5 dB at 44.1, 48 and 96 kHz
- band isolation
- the compressor's static gain within ±1 dB at −60, −40 and −15 dBFS
- the limiter never exceeds the ceiling, including the absolute ceiling of −1 dBFS
- transparent with unity parameters
- output gain within ±0.5 dB, and +12 dB of output gain cannot drive the output above the ceiling
- the Music time constants give a longer release than Speech (measured recovery after a step)
- meters: a −20 dBFS sine gives RMS −20 ±0.5 and peak −20 ±0.5, and silence gives ≤ −100
- linear gain per band
- the ears are processed independently
- the MPO holds with high gain
- no clicks on parameter changes
- NaN and Inf are zeroed
- large blocks are split correctly
- silence stays silent
- triple buffer: the latest value wins and it works under concurrency
- no allocations in `process` (`assert_no_alloc`)
- release benchmark: 512 stereo frames at 48 kHz, including denormal input, take under 10 % of the buffer time

**`swift test`** (in `AudioCurveCore`):
- validation
- the prescription rule: normal hearing, HL 100 (60/25/4.5), the CR clamp, strength 0 and 0.5, the low-frequency correction, trim, interpolation and independent ears
- the `gainDB` curve
- `PluginState` round trip, corrupt data and unknown version
- the editing logic (rounding to 5 or 1 dB, limits, adding and deleting with at least 4 points, neighbouring point, ear switch, typed value, trim) and the auto-level calculation

**Integration:** `auval -v aufx hcmp Lnds` must give `AU VALIDATION SUCCEEDED`.

**Manual** (`docs/manual-test.md`):
1. A/B in the host app.
2. SoundSource system-wide.
3. SoundSource per app.
4. Switching output in SoundSource.
5. The profile survives a restart of SoundSource.
6. 44.1 and 96 kHz via Audio MIDI Setup.
7. A listening test of at least 10 minutes without crackles.

## 8. Risks

- **Signing:** SoundSource may refuse to load a locally signed AUv3. The first implementation step is therefore a passthrough plugin, verified in SoundSource before any DSP is built.
- **The hardware sets a limit:** in bands above about 100 dB HL, audibility cannot be reached. This will be addressed later with frequency lowering (v2).
- **The open prescription rule** is not clinically validated. Strength and trim are the tools for adjustment.
- **AirPods' own accommodations** (Headphone Accommodations) can stack on top. They should be off during testing.
