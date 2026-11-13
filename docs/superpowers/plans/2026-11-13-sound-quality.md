# AudioCurve 2.1 Sound Quality Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Less distortion and a warmer sound: a measurement tool with a 2.0.0 baseline, a two-time-constant compressor, separate speech and music prescriptions blended by Auto, a gentler true-peak limiter and a Tone control.

**Architecture:** The measurement code lives in `dsp/tests/support/measure/` and is shared by the `measure` example (report) and the `measurements` test (regression against the committed baseline). The kernel gains a second fitting set and a speech weight; the compressor's gain curve is a blend of both fittings. The prescriptions and Tone are computed in Swift (`FittingRule`) and mirrored in Rust only for measurement, checked by a shared case table.

**Tech Stack:** Rust 2024 (`dsp/`, dev-dependency `serde_json`), Swift 6 (`Packages/AudioCurveCore`, `AU/`), XcodeGen, Makefile.

**Spec:** `docs/superpowers/specs/2026-11-13-sound-quality-design.md`

## Global Constraints

- Real-time code (`dsp/src/`) never allocates, locks or logs in `process`; `assert_no_alloc` tests stay green.
- Sample rates 16–96 kHz keep working; all time constants are converted with the actual `fs`.
- Test signals are generated in code; never commit audio files or a real person's audiogram. Example audiograms are the two in Task 1.
- English only in code, docs and screenshots. New UI strings get all 8 languages in the String Catalogs.
- Views use `@ViewState`, never `@State` (the Makefile rejects it). `make release` must keep building.
- Bundle IDs, the profile path and existing AU parameter addresses 0–6 do not change.
- Commits use `19973950+paddan@users.noreply.github.com` and end with the `Co-Authored-By` line. No pushes or tags without the user's approval.
- Version 2.1.0 (`project.yml` `MARKETING_VERSION`).

## Review Focus

1. Sample rate 44.1 and 96 kHz: the dual detector, the limiter's lookahead ring and the true-peak interpolator must size from `fs`; a test runs the limiter at 44.1 and 96 kHz.
2. Strength 0 and Tone 100 % together: every gain must stay ≥ 0 and finite, CR within its clamp; covered by a Swift test.
3. A state saved by 2.0 (no `tone`, old 640-byte params never persisted, but `fullState` without key 7) must load with Tone 0; Swift test.
4. Silence after loud music: the slow detector's 1.5 s release must not leave the gain stuck low; the dual-detector test checks recovery to within 1 dB of the static gain after 5 × release.
5. Mode switch Speech → Music while playing: the fitting weight switches instantly outside Auto; the 20/3 ms gain smoothing must keep the step click-free; a kernel test bounds the sample-to-sample output step.

---

### Task 1: Measurement tool and 2.0.0 baseline

**Files:**
- Create: `dsp/tests/support/measure/mod.rs`, `signals.rs`, `metrics.rs`, `prescription.rs`, `report.rs`
- Create: `dsp/examples/measure.rs`, `dsp/tests/measurements.rs`
- Create: `dsp/measurements/baseline-2.0.0.json`, `dsp/measurements/report.md`, `dsp/measurements/report.json`
- Modify: `dsp/Cargo.toml` (dev-dependency `serde_json = "1"`), `.gitignore` (`dsp/measurements/wav/`)
- Delete: `dsp/examples/thd.rs`, `sib.rs`, `pump.rs`, `limstat.rs`

**Interfaces:**
- Produces:
  - `measure::Audiogram { left: &'static [(f64, f64)], right: &'static [(f64, f64)] }` and `measure::MILD`, `measure::STEEP`:
    - MILD both ears: (125,10),(250,10),(500,15),(1000,20),(2000,30),(4000,40),(8000,45)
    - STEEP left: (125,20),(250,20),(500,30),(1000,45),(2000,60),(3000,70),(4000,80),(6000,90),(8000,90); right = left minus 20 dB, floored at 0.
  - `enum Mode { Speech, Music, Auto }`
  - `prescription::fittings(a: &Audiogram, ear: Ear, mode: Mode) -> [BandFitting; 12]`. In this task it is the 2.0 rule (copy `pump_fit` from `tests/support/mod.rs`, strength 1); Task 4 replaces it.
  - `measure::params(a: &Audiogram, mode: Mode) -> Params`: MPO −3 dBFS, output 0, input 0, limiter on, active. Speech: attack 5, release 50, auto 0. Music: 10/200, auto 0. Auto: 10/200, auto 1. Task 2 changes it.
  - `measure::run_all(write_wav: Option<&Path>) -> BTreeMap<String, f64>` with keys `"{mild|steep}/{speech|music|auto}/{metric}"`.
  - `report::write(map, dir)` writes `report.json` (flat object, values rounded to 0.01) and `report.md` (one table per audiogram).
  - `report::LOWER_IS_BETTER: &[&str]` metric-name suffixes checked by the regression test.
- Metrics at fs 48 kHz, through `Handle` (`set_params`, `process` in 512-frame blocks), discarding the first 1 s:
  - `thdn_{250|1000|4000}_{m40|m20|m6}`: sine at that frequency and level, both ears, Speech and Music. Fit and remove the fundamental by least squares over the last 1 s; residual power relative to fundamental, dB.
  - `imd_smpte_{m20|m6}`: 60 Hz + 7 kHz at 4:1 amplitude; power of the 7 kHz ± n·60 Hz sidebands (n = 1..4) relative to the 7 kHz component, dB. `imd_ccif_{m20|m6}`: 19 + 20 kHz equal amplitude; power at 1 kHz and 18/21 kHz relative to the tones, dB. Level is the peak of the sum.
  - `gainmod_hf`: pink noise at −20 dBFS RMS, amplitude-modulated by a sum of 2, 5 and 8 Hz sines (±10 dB total), 4 s. Run `Filterbank` + one `BandCompressor` per band directly with the kernel's attack/release for the mode; std of the gain (dB) per band over the last 3 s, mean over bands with center > 2 kHz. Speech and Music.
  - `safety_active_pct`, `safety_mean_db`, `safety_max_db`: dense signal = pink noise at −10 dBFS RMS plus a 30 ms decaying noise burst every 250 ms peaking at −0.5 dBFS, 6 s. Per 512-frame block read `Meters::reduction_db` (max of ears); active = block reduction > 0.1 dB. Speech, Music and Auto.
  - `isp_over_db`: highest 4×-oversampled true peak of the output (64-tap-per-phase Kaiser windowed sinc, β = 8) minus the MPO ceiling, on the dense signal and on a full-scale 11 025 Hz sine at 45° phase (worst-case inter-sample peak). Speech and Music.
  - `tilt_db_per_oct`, `lowhigh_diff_db`: LTASS-shaped noise at −20 dBFS (pink noise through a 1st-order high-pass at 100 Hz and a 2nd-order low-pass at 4 kHz is the approximation used). Welch spectrum of output minus input; least-squares slope over 250 Hz–6 kHz in dB per octave; mean of 2–4 kHz minus mean of 250–500 Hz. Report only, not in `LOWER_IS_BETTER`.
- `report::LOWER_IS_BETTER = ["thdn_", "imd_", "gainmod_hf", "safety_", "isp_over_db"]`.

- [ ] **Step 1: Write `dsp/tests/measurements.rs`**

```rust
#[path = "support/measure/mod.rs"] mod measure;
#[test] fn no_metric_worse_than_2_0_0() {
    let base: BTreeMap<String, f64> = serde_json::from_str(include_str!("../measurements/baseline-2.0.0.json")).unwrap();
    let now = measure::run_all(None);
    for (k, b) in &base {
        if !measure::report::LOWER_IS_BETTER.iter().any(|s| k.split('/').last().unwrap().starts_with(s)) { continue; }
        let tol = if k.contains("safety_active_pct") { 1.0 } else { 0.5 };
        assert!(now[k] <= b + tol, "{k}: {} > baseline {b}", now[k]);
    }
}
#[test] fn metrics_are_finite_and_complete() {
    let now = measure::run_all(None);
    assert_eq!(now.len(), 2 * (2 * (9 + 4 + 1 + 3 + 1 + 2) + 3));
    assert!(now.values().all(|v| v.is_finite()));
}
```

- [ ] **Step 2: Run** `cd dsp && cargo test --release --test measurements` — Expected: FAIL (module `measure` missing).
- [ ] **Step 3: Implement the `measure` module and `examples/measure.rs`** (`cargo run --release --example measure [--wav]` writes `dsp/measurements/report.{md,json}`; with `--wav`, input and output WAV pairs per metric to `dsp/measurements/wav/`).
- [ ] **Step 4: Generate the baseline** with `cargo run --release --example measure` and copy `report.json` to `baseline-2.0.0.json`. Check `report.md` by eye: the steep audiogram must show larger `gainmod_hf` than mild.
- [ ] **Step 5: Run** `cargo test --release` — Expected: all pass, including both measurement tests.
- [ ] **Step 6: Delete the four old examples**, run `cargo build --release --examples` (Expected: builds), commit `feat(dsp): measurement tool and 2.0.0 baseline`.

### Task 2: Speech and music fittings in the kernel

**Files:**
- Modify: `dsp/src/params.rs`, `dsp/src/kernel.rs`, `dsp/src/compressor.rs`, `dsp/tests/layout.rs`, `dsp/tests/kernel.rs`, `dsp/tests/compressor.rs`
- Modify: `Packages/AudioCurveCore/Sources/CAudioCurveDSP/include/audiocurve_dsp.h`, `.../FittingRule.swift`, `Tests/.../LayoutTests.swift`, `Tests/.../FittingRuleTests.swift`
- Modify: `dsp/tests/support/measure/mod.rs` (`params`), spec §4.2 (see ruling below)

**Interfaces:**
- Produces:
  - `Params` field order: `left, right` (speech fitting), `music_left, music_right: [BandFitting; 12]`, `mpo_dbfs, output_gain_db, speech_weight, limiter_enabled, input_gain_db, auto_mode, active` (all `f64`). `attack_ms`/`release_ms` are removed. Size 1208 bytes.
  - `speech_weight`: used when `auto_mode < 0.5`; 1 = Speech, 0 = Music. In Auto the classifier output replaces it.
  - `BandCompressor::analyze(&mut self, x: &[f32], speech: &BandFitting, music: &BandFitting, weight: f64, attack_ms: f64, release_ms: f64, gain_out: &mut [f32], level_out: &mut [f32])`: static gain = `weight * gain_db(speech, L) + (1 - weight) * gain_db(music, L)`.
  - Kernel time constants: `MUSIC_ATTACK_MS = 10.0`, `MUSIC_RELEASE_MS = 200.0` join the existing speech constants; attack/release = `log_blend(music, speech, w)` with `w` = classifier output (Auto) or `speech_weight`.
  - Swift: `FittingRule.params(...)` sets `speech_weight` (Speech 1, Music 0, Auto 0) and, until Task 4, the same fitting in both sets. `CompressionMode.attackMS`/`releaseMS` are removed.
- Ruling to write into spec §4.2: replace "Swift sends the selected fitting in both sets" by "`speech_weight` selects the fitting and the time constants outside Auto"; it lets one weight drive fitting and timing alike.

- [ ] **Step 1: Write failing tests**
  - `layout.rs`: `params_is_1208_bytes`; `LayoutTests.swift`: `MemoryLayout<ac_params>.size == 1208`.
  - `compressor.rs`: `blend_weight_selects_fitting` — with speech fitting 30/10 dB CR 3 and music 10/10 CR 1, a −30 dBFS sine settles to the speech static gain at weight 1, the music static gain at weight 0, and their mean (±0.1 dB) at 0.5.
  - `kernel.rs`: `identical_fittings_match_single_fitting` — same output (max abs diff < 1e-6) for weights 0, 0.5 and 1 when both sets are equal; `mode_switch_is_click_free` — switching `speech_weight` 0 → 1 mid-signal (different fittings, −20 dBFS 1 kHz sine) gives no sample-to-sample output step larger than 2 × the largest step of the steady signal.
- [ ] **Step 2: Run** `cargo test --release` and `swift test` — Expected: FAIL on the new tests (field and signature missing).
- [ ] **Step 3: Implement** the `Params`, header, compressor, kernel and `FittingRule.params` changes.
- [ ] **Step 4: Run** `cargo test --release` (Expected: all pass, `measurements` unchanged within tolerance) and `swift test` (Expected: all pass). `xcodebuild` Debug builds.
- [ ] **Step 5: Commit** `feat(dsp): speech and music fittings blended by a speech weight`.

### Task 3: Compressor with two time constants

**Files:**
- Modify: `dsp/src/compressor.rs`, `dsp/src/kernel.rs`, `dsp/tests/compressor.rs`, `dsp/tests/support/measure/metrics.rs`, `dsp/measurements/report.*`

**Interfaces:**
- Consumes: `analyze` from Task 2.
- Produces:
  - `pub struct Timing { pub slow_attack_ms: f64, pub slow_release_ms: f64, pub fast_attack_ms: f64, pub fast_release_ms: f64 }`
  - `pub const MUSIC_TIMING: Timing = Timing { slow_attack_ms: 50.0, slow_release_ms: 1500.0, fast_attack_ms: 2.0, fast_release_ms: 50.0 }`, `pub const SPEECH_TIMING: Timing = Timing { 20.0, 300.0, 2.0, 50.0 }` (field order as declared), `pub const FAST_JUMP_DB: f64 = 8.0`, `pub fn blend_timing(speech_weight: f64) -> Timing` (log blend per field, Music at 0).
  - `analyze(&mut self, x, speech, music, weight, timing: &Timing, gain_out, level_out)` replaces the attack/release arguments; the kernel passes `blend_timing(w)`. `MUSIC_ATTACK_MS`/`MUSIC_RELEASE_MS` and `SPEECH_ATTACK_MS`/`SPEECH_RELEASE_MS` are removed.
- Detector per sample, after the existing 5 ms power average `l` (dB):
  - fast level `lf` smoothed with fast attack/release;
  - slow level `ls` smoothed with slow attack/release, then `ls = max(ls, lf − FAST_JUMP_DB)`;
  - static gain from `ls`; the 20/3 ms gain smoothing and the MPO detector are unchanged.

- [ ] **Step 1: Write failing tests in `compressor.rs`**
  - `slow_level_ignores_10hz_modulation`: −30 dBFS 1 kHz sine with ±6 dB 10 Hz amplitude modulation, CR 3 fitting; std of the gain over the last 1 s < 0.5 dB (today about 3 dB).
  - `fast_path_catches_a_step`: 2 s at −40 dBFS then +12 dB; the gain is within 1 dB of the post-step static gain 20 ms after the step.
  - `recovers_after_loud_music`: 2 s at −10 dBFS then −40 dBFS; within 1 dB of the −40 dBFS static gain after 5 × `slow_release_ms` (Music timing).
  - `timing_blend_ends`: `blend_timing(0.0) == MUSIC_TIMING`, `blend_timing(1.0) == SPEECH_TIMING`.
- [ ] **Step 2: Run** `cargo test --release --test compressor` — Expected: FAIL.
- [ ] **Step 3: Implement** the detector, `Timing` and the kernel call.
- [ ] **Step 4: Run** `cargo test --release` — Expected: all pass. Then `cargo run --release --example measure`: `steep/*/gainmod_hf` must be ≤ 0.5 × its baseline value; if not, tune `slow_*` within attack 20–80 ms and release 0.3–2 s and `FAST_JUMP_DB` within 6–12 dB, record the values chosen in the ledger, rerun.
- [ ] **Step 5: Commit** code and the updated `report.md`/`report.json`: `feat(dsp): compressor with slow and fast level detectors`.

### Task 4: Speech and music prescriptions and Tone in FittingRule

**Files:**
- Create: `Packages/AudioCurveCore/Tests/AudioCurveCoreTests/Fixtures/prescription-cases.json`, `dsp/tests/prescription.rs`
- Modify: `Packages/AudioCurveCore/Sources/AudioCurveCore/FittingRule.swift`, `Package.swift` (test resources), `Tests/.../FittingRuleTests.swift`, `AU/PluginViewModel.swift`, `AU/AudioCurveAudioUnit.swift`, `dsp/tests/support/measure/prescription.rs`, `dsp/measurements/report.*`

**Interfaces:**
- Produces (Swift):
  - `public enum Prescription: Sendable { case speech, music }`
  - `FittingRule.bands(for: HearingProfile, ear: Ear, strength: Double, tone: Double = 0, prescription: Prescription) -> [BandFitting]`
  - `FittingRule.params(profile:strength:tone:mpoDBFS:outputGainDB:mode:compression:limiter:inputGainDB:active:)` — new `tone: Double = 0` after `strength`; fills `left/right` with `.speech` and `music_left/music_right` with `.music`.
  - `FittingRule.blend(_ speech: [BandFitting], _ music: [BandFitting], weight: Double) -> [BandFitting]` (linear blend of gSoft and gLoud, CR recomputed by the 60 dB rule, unclamped) for display.
- Rules (spec §4.2), per band, `HL` interpolated, `t` = tone clamped 0…1, `s` = strength clamped 0…1:
  - `HLe = HL ≤ 60 ? HL : 60 + 0.5·(HL − 60)`; `corr` = 10 (f ≤ 250), 5 (f = 500), else 0 (Speech only).
  - Base: Speech `0.6·HLe − corr` / `0.25·HLe − corr`; Music `0.45·HLe` / `0.3·HLe` (f ≤ 2000) or `0.2·HLe` (f > 2000).
  - Tone: add `5t` (f ≤ 250), `3t` (f = 500), `1t` (f = 750 or 1000) to both gains; subtract `6t` from the loud gain for f > 2000.
  - `g = (max(0, base + tone) + trim) · s` for each of soft and loud; then `CR = 60 / (60 − (gSoft − gLoud))` clamped to 1…3 (Speech) or 1…2 (Music); `CR = max` when the denominator ≤ 0.
- Rust mirror: `prescription::fittings(a, ear, mode, tone)` implements the same rules (Auto uses both sets), and `measure::params` fills both sets.
- Fixture rows `{ "prescription": "speech|music", "f": Double, "hl": Double, "tone": Double, "soft": Double, "loud": Double, "cr": Double }` (strength 1, trim 0), at least these:

| prescription | f | HL | tone | soft | loud | CR |
|---|---|---|---|---|---|---|
| speech | 1000 | 0 | 0 | 0 | 0 | 1 |
| speech | 1000 | 40 | 0 | 24 | 10 | 1.3043 |
| speech | 1000 | 60 | 0 | 36 | 15 | 1.5385 |
| speech | 1000 | 80 | 0 | 42 | 17.5 | 1.6901 |
| speech | 1000 | 100 | 0 | 48 | 20 | 1.875 |
| speech | 1000 | 120 | 0 | 54 | 22.5 | 2.1053 |
| speech | 250 | 40 | 0 | 14 | 0 | 1.3043 |
| speech | 4000 | 100 | 1 | 48 | 14 | 2.3077 |
| music | 1000 | 60 | 0 | 27 | 18 | 1.1765 |
| music | 4000 | 100 | 0 | 36 | 16 | 1.5 |
| music | 250 | 40 | 0 | 18 | 12 | 1.1111 |
| music | 250 | 40 | 1 | 23 | 17 | 1.1111 |

- [ ] **Step 1: Write failing tests**
  - Swift `prescriptionCasesMatchFixture` (flat audiogram at `hl`, band at `f`, ±0.001) and Rust `prescription_cases_match_fixture` reading the same file via `include_str!("../../Packages/AudioCurveCore/Tests/AudioCurveCoreTests/Fixtures/prescription-cases.json")`.
  - Swift `speechRatioAtMost3`, `musicRatioAtMost2AndNoBassCut` over HL 0…120 in 10 dB steps and every band.
  - Swift `toneZeroIsBase`, `toneKeepsSoftTrebleAndLowersLoudBy6`.
  - Swift `strengthZeroToneFullIsFiniteAndNonNegative` (Review Focus 2).
- [ ] **Step 2: Run** `swift test` and `cargo test --release --test prescription` — Expected: FAIL.
- [ ] **Step 3: Implement** the Swift rules, `params` and `blend`; update the two callers in `AU/` (`tone: 0` until Task 6); implement the Rust mirror.
- [ ] **Step 4: Run** `swift test`, `cargo test --release` — Expected: all pass, `measurements` included. Run `measure` and commit the report.
- [ ] **Step 5: Commit** `feat: speech and music prescriptions with Tone`.

### Task 5: Gentler true-peak limiter

**Files:**
- Modify: `dsp/src/limiter.rs`, `dsp/tests/limiter.rs`, `dsp/tests/kernel.rs` (latency), `dsp/measurements/report.*`

**Interfaces:**
- Produces: `Limiter` keeps `new(fs, max_frames)`, `process`, `process_bypass`, `last_reduction_db`, `latency_frames`, `reset`.
- Constants: `LOOKAHEAD_S = 0.005`, `RELEASE_S = 0.150`, `KNEE_DB = 3.0`, `TP_GUARD_DB = 0.2`, `OVERSAMPLE = 4`, `TP_TAPS_PER_PHASE = 16` (Kaiser windowed sinc, β = 8, coefficients computed in `new`).
- Algorithm per channel:
  - true peak at sample `t − TP_TAPS_PER_PHASE/2`: max of `|x|` and the 3 interpolated values after it;
  - `need` = soft-knee gain for that peak against `c · 10^(−TP_GUARD_DB/20)`;
  - `h` = minimum of `need` over the last `L + 1` values (`L` = lookahead in samples), via a preallocated monotonic deque;
  - `gb` = moving average of `h` over the last `L` values (running sum);
  - `g = gb` if `gb < g_prev`, else `g_prev + (gb − g_prev)·(1 − e^(−1/(RELEASE_S·fs)))`;
  - output = input delayed by `L + TP_TAPS_PER_PHASE/2` times `g`, then the existing NaN-safe hard clamp to `c`.
- `latency_frames() = L + TP_TAPS_PER_PHASE/2`.

- [ ] **Step 1: Write failing tests in `limiter.rs`**
  - `true_peak_stays_below_ceiling`: full-scale 11 025 Hz sine at 45° and pink noise with bursts to +6 dBFS, ceiling −3 dBFS; 4× oversampled output (64-tap measurement filter from the measure support) ≤ ceiling.
  - `hard_clamp_never_engages`: on the same signals the pre-clamp value never exceeds `c`.
  - `attack_is_smooth`: on a step from −20 to +6 dBFS, no gain change between consecutive samples > `1/L` of the total reduction (+1e-6).
  - `latency_matches_delay` at 44.1, 48 and 96 kHz: an impulse comes out exactly `latency_frames()` later (Review Focus 1).
- [ ] **Step 2: Run** `cargo test --release --test limiter` — Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test --release` — Expected: all pass. Run `measure`: `isp_over_db ≤ 0` everywhere; `safety_active_pct` below baseline. Commit the report.
- [ ] **Step 5: Commit** `feat(dsp): true-peak limiter with smooth lookahead attack`.

### Task 6: Tone parameter and control

**Files:**
- Modify: `AU/Parameters.swift`, `AU/PluginViewModel.swift`, `AU/AudioCurveAudioUnit.swift`, `AU/UI/ControlsPanel.swift`, `AU/Localizable.xcstrings` (or the catalog the AU strings live in), `Packages/AudioCurveCore/Tests/AudioCurveCoreTests/PluginStateTests.swift`

**Interfaces:**
- Consumes: `FittingRule.params(...tone:...)` from Task 4.
- Produces: `ParamAddress.tone = 7`, identifier `"tone"`, name "Tone", 0–100 %, default 0; `ParamValues.tone: Double = 0`. Both `FittingRule.params` call sites pass `tone: model.tone / 100`; `PluginViewModel.fitting(_:)` passes it too.
- UI: `SliderRow(title: "Tone", …, range: 0...100, step: 1, defaultValue: 0)` directly below Strength, with "Clear" and "Warm" as the ends (`minimumValueLabel`/`maximumValueLabel` or the row's existing end-label pattern). Strings "Tone", "Clear", "Warm" in all 8 languages.

- [ ] **Step 1: Write failing tests**
  - `PluginStateTests`: `stateWithoutToneLoadsAsZero` (a `fullState` dictionary with only keys 0–6 restores `tone == 0`; Review Focus 3).
  - A test that the parameter tree contains address 7 with identifier `tone` (in the AU target's existing test location if any; otherwise in `PluginStateTests` through the shared `ParamValues` defaults).
- [ ] **Step 2: Run** `swift test` — Expected: FAIL.
- [ ] **Step 3: Implement**, `xcodegen generate`, build Debug.
- [ ] **Step 4: Run** `swift test`, `xcodebuild … build`, `make release` — Expected: pass and build.
- [ ] **Step 5: Commit** `feat(au): Tone control (Clear to Warm)`.

### Task 7: Gain view shows both prescriptions

**Files:**
- Modify: `AU/PluginViewModel.swift`, `AU/UI/GainView.swift`

**Interfaces:**
- Consumes: `FittingRule.bands(...prescription:)`, `FittingRule.blend` (Task 4), `Meters.speech`.
- Produces: `PluginViewModel.fitting(_ ear: Ear) -> [BandFitting]` returns the fitting in effect: Speech or Music mode → that prescription; Auto → `blend(speech, music, weight: meters.speech)`. New `fittings(_ ear: Ear) -> (speech: [BandFitting], music: [BandFitting])`.
- GainView keeps its bars for `fitting(ear)`; per band it adds two thin horizontal ticks at the speech and music soft gains (secondary color, 1 pt; speech solid, music dashed). The axis range (`gainRange`) covers all three. Trim dragging is unchanged.
- Ruling: the spec's "thin lines" become per-band ticks because GainView draws bars, not curves.

- [ ] **Step 1: Implement**; no unit test (view code). Build Debug; open the window with the example profile and check by eye in Speech, Music and Auto: ticks visible, bars move with the classifier in Auto.
- [ ] **Step 2: Run** `swift test`, `make release` — Expected: pass and build.
- [ ] **Step 3: Commit** `feat(au): gain view shows the speech and music prescriptions`.

### Task 8: Docs, version and final report

**Files:**
- Modify: `README.md` (Features: two prescriptions, Tone, measurement tool and `cargo run --release --example measure`; latency "5 ms" → the kernel's new value), `docs/manual-test.md` (section "Sound quality (2.1)" with the four manual checks from spec §6), `project.yml` (`MARKETING_VERSION: "2.1.0"`), `dsp/measurements/report.*`

- [ ] **Step 1:** Run `cargo run --release --example measure --wav`; check every success criterion of spec §1 against `report.md` vs `baseline-2.0.0.json`; write the comparison (baseline, 2.1, change) as a table at the top of `report.md`.
- [ ] **Step 2:** Update README, manual test and version; `xcodegen generate`.
- [ ] **Step 3: Run** `cargo test --release`, `swift test`, `make release` — Expected: all pass, builds.
- [ ] **Step 4: Commit** `docs: AudioCurve 2.1 sound quality`.
