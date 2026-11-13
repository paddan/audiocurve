# AudioCurve 2.1: less distortion, a warmer sound

Date: 2026-11-13
Status: draft for review

## 1. Goal

AudioCurve 2.0 can sound distorted on loud or dense music, the treble can be sharp, and the sound can be thin. Version 2.1 makes the processing cleaner and warmer without taking audibility away from soft treble sounds.

Success means, measured with the steep example audiogram (section 3.1) against the 2.0.0 baseline:

- The gain modulation in the treble bands (above 2 kHz) is at most half of 2.0.0's.
- THD+N and intermodulation at −6 dBFS input are lower than in 2.0.0.
- No inter-sample peak (4× oversampled) exceeds the ceiling (MPO).
- The safety stage (band budget and limiter) is active for a smaller share of the time on the dense test signal.
- No metric in the report is worse than in 2.0.0.
- The user confirms by listening to the A/B files and to real music on AirPods.

Out of scope: clearer speech in background noise (noise reduction or speech enhancement) is the next sub-project. Frequency lowering, headphone correction and a built-in hearing test are later sub-projects.

## 2. Approach

Measure first, then fix. A measurement tool turns the impressions "distorted", "sharp" and "thin" into numbers and A/B files. Each fix is accepted only if the numbers improve and nothing else gets worse. The tool stays in the repo and is reused for the noise-reduction sub-project.

Rejected alternatives:

- Applying the fixes without measuring. Faster, but there is no way to know which change helped, or to catch a later regression.
- Only switching to an established prescription. It would likely sound warmer, but it does not remove the distortion from the compressors and the limiter.

## 3. Measurement tool

### 3.1 Tool and example audiograms

`cargo run --release --example measure` in `dsp/` runs the kernel (`Dsp`) offline and writes a report to `dsp/measurements/report.md` and `report.json`, plus A/B WAV files to `dsp/measurements/wav/` (git-ignored).

It uses two fixed, made-up audiograms defined in the tool:

- **Mild:** gently sloping, 10 dB HL at 250 Hz to 45 dB HL at 8 kHz, both ears.
- **Steep:** asymmetric, left ear 20 dB HL at 250 Hz falling to 90 dB HL at 6–8 kHz, right ear about 20 dB better. It exercises the high-gain case without being any real person's audiogram.

The tool builds the band fittings with a Rust copy of the prescription (section 4.2), checked against the Swift rule by a shared table of expected values (section 6).

Each metric is reported per audiogram and per mode (Speech, Music, and Auto on a mixed signal).

### 3.2 Metrics

All test signals are generated in code, so no copyrighted audio is needed.

1. **THD+N:** sines at 250 Hz, 1 kHz and 4 kHz, at −40, −20 and −6 dBFS input, after the kernel's settling time. The residual after removing the fundamental, as dB relative to the fundamental.
2. **Intermodulation:** SMPTE (60 Hz + 7 kHz, 4:1) and CCIF (19 kHz + 20 kHz) at −20 and −6 dBFS. The IM products, as dB relative to the signal.
3. **Gain modulation:** one second of pink noise with music-like dynamics (amplitude modulated at 2–8 Hz, ±10 dB). For each band, the standard deviation of the applied WDRC gain (dB). The report gives the mean over the bands above 2 kHz.
4. **Safety-stage activity:** a dense, loud test signal (pink noise plus percussive bursts, peaks near 0 dBFS). The share of time the budget or the limiter reduces gain, and the mean and deepest reduction.
5. **Inter-sample peaks:** the output of the dense signal and of full-scale square-like waveforms, upsampled 4×. The highest true peak relative to the ceiling.
6. **Tone balance:** LTASS-shaped noise (the long-term average speech spectrum) at −20 dBFS. The output spectrum's slope in dB per octave, and the level difference between 250–500 Hz and 2–4 kHz.

The existing diagnostics `thd`, `sib`, `pump` and `limstat` are folded into `measure` and deleted. `render` and `classify` stay.

### 3.3 Baseline and regression check

The first run, on the unchanged 2.0.0 kernel, is committed as `dsp/measurements/baseline-2.0.0.json`. A test (`cargo test --release --test measurements`) recomputes the metrics and fails if any is worse than the baseline by more than a small tolerance (0.5 dB, or 1 percentage point for activity). After each fix the committed report is updated, and the test compares against the baseline, not against the previous fix.

## 4. Fixes

They are applied in this order, each with a before/after report.

### 4.1 Compressor with two time constants

Today the per-band gain follows the level detector with 5–10 ms attack, so in the treble bands it follows syllables and transients and modulates the signal.

New detector per band:

- A **slow** level, smoothed in dB: about 50 ms attack and 1–2 s release for Music; about 20 ms attack and 300 ms release for Speech. It sets the gain.
- A **fast** level with the current time constants. When it exceeds the slow level by more than about 8 dB (a sudden loud sound), the gain follows the fast level down, then returns to the slow level's gain as the fast level falls (the fast path only follows the fast level down after the jump, so the gain never rises again while a sustained sound such as an /s/ lasts). The jump never moves the slow level itself. (Amended after the 2.1 listening test: returning with the slow release held the treble gain down after every plosive and /s/ onset in speech, which was heard as clipped, choppy speech.)
- Auto glides both sets of time constants with the classifier as today.

The exact time constants and the 8 dB threshold are tuned with the measurements (metric 3 must halve without metric 4 getting worse). The MPO detector (held band peak) is unchanged.

### 4.2 Two prescriptions: speech and music

`FittingRule` computes two fittings per ear. Strength and the per-band trim apply to both.

- **Speech (NAL-inspired).** It follows principles published for NAL-NL2, without claiming to be NAL-NL2, whose formula is not public:
  - less gain where the loss is very large, since the benefit falls off there;
  - weight on 1–4 kHz, where speech intelligibility is decided;
  - overall loudness for speech at or below what a normal-hearing listener perceives;
  - compression range widened so the ratio is normally at most about 3:1.

The rules, per band at center frequency `f` with interpolated threshold `HL` (dB HL), before Strength and trim:

- Effective loss: `HLe = HL` up to 60 dB HL; above, only half the excess counts: `HLe = 60 + 0.5 · (HL − 60)`.
- Low-frequency correction `corr` as today: 10 dB at 250 Hz and below, 5 dB at 500 Hz, 0 above.
- Compression ratio from a 60 dB input range (today 45): `CR = 60 / (60 − (gSoft − gLoud))`, clamped to 1…3 for Speech and 1…2 for Music.

| | gSoft | gLoud |
|---|---|---|
| Speech | `0.6 · HLe − corr` | `0.25 · HLe − corr` |
| Music, `f` ≤ 2 kHz | `0.45 · HLe` | `0.3 · HLe` |
| Music, `f` > 2 kHz | `0.45 · HLe` | `0.2 · HLe` |

All gains are floored at 0 dB. For example, at 100 dB HL: Speech 48/20 dB (CR 1.9), Music above 2 kHz 36/16 dB (CR 1.5); 2.0 gives 60/25 dB (CR 4.5).

- **Music (near-linear).** Ratio at most 2:1, no low-frequency reduction, and less gain for loud sounds above 2 kHz than Speech.

How the kernel uses them:

- `Params` gains a second set of band fittings per ear (`music_left`, `music_right`) and `speech_weight`, and loses `attack_ms`/`release_ms`; the existing `left` and `right` hold the speech fitting.
- In Auto, the gain curve per band is a blend in dB of the two fittings' static curves, weighted by the classifier output (0 = music, 1 = speech), the same smoothed value that already glides the time constants. The glide takes a few seconds, so nothing jumps.
- `speech_weight` selects the fitting and the time constants outside Auto (1 = Speech, 0 = Music); in Auto the classifier output takes its place. One weight drives fitting and timing alike.
- The C header, the FFI layout tests and the Swift parameter block are updated.

### 4.3 Gentler safety stage

- The limiter's attack becomes a smooth gain ramp over the whole lookahead window, about 5 ms, instead of a 0.5 ms one-pole.
- Peak detection is 4× oversampled, so inter-sample peaks are caught.
- The hard clamp after the limiter stays as the guarantee; on the test signals it must not engage.
- The reported latency rises by about 3 ms, to about 8 ms for the plugin. The app's render path and the AU's `latency` follow the kernel's reported value.

### 4.4 Tone (Clear ↔ Warm)

- A new AU parameter, `tone`, address 7, 0–100 %, default 0. Like Strength it is saved in the plugin's state, not in the profile file, and hosts can automate it.
- It adjusts both prescriptions in `FittingRule`:
  - Toward Warm, low and mid frequencies (up to about 1 kHz) get more gain, up to about +5 dB at 250 Hz.
  - Loud-level gain above 2 kHz falls by up to about 6 dB at 100 %.
  - Soft-level gain above 2 kHz is unchanged, so soft consonants stay audible.
- Tone = 0 gives the prescriptions unchanged.

## 5. User interface

- **Controls panel:** a Tone slider next to Strength, labelled "Tone" with "Clear" and "Warm" at the ends.
- **Gain view:** the curve in effect now as the main line, with the speech and music prescriptions as thin lines, so the difference and Auto's current position are visible.
- **Menu bar panel:** unchanged.
- **Localization:** new strings in all 8 languages.
- **Compatibility:** a saved state without `tone` loads with Tone = 0. Profile files are unchanged.

## 6. Tests

Automatic, Rust:

- **Dual detector:** a 10 Hz amplitude modulation leaves the gain nearly constant; a +12 dB step makes the gain fall within the fast attack and recover with the slow release.
- **Blend:** classifier output 0 gives exactly the music curve, 1 exactly the speech curve; equal fittings give the same output as a single fitting.
- **Limiter:** on the worst-case signals, the 4× oversampled output never exceeds the ceiling; the reported latency matches the delay.
- **FFI layout** for the new `Params`.
- **Measurement regression** (section 3.3).

Automatic, Swift:

- Speech prescription: the table in 4.2 for HL 0, 40, 60, 80, 100 and 120 dB HL, and ratio at most 3:1.
- Music prescription: ratio at most 2:1, and no low-frequency reduction.
- Tone = 0 equals the base prescription. Tone = 1 leaves the soft gain above 2 kHz unchanged and lowers the loud gain there by up to 6 dB.
- The Rust copy of the prescription used by the measurement tool matches the Swift rule on a shared table of cases.
- The parameter tree contains `tone`, and a saved state without it loads with Tone = 0.

The existing Rust and Swift tests stay green.

Manual, added to `docs/manual-test.md`:

- Listen to the A/B files from the report: before and after each fix.
- Dense rock or orchestral music at high volume on AirPods: no audible distortion; the Safety meter moves less than in 2.0.
- Speech podcast and a film with music: Auto moves between the prescriptions without audible jumps.
- Tone from Clear to Warm: fuller and softer, soft consonants still audible.

## 7. Release

Version 2.1.0. The README's feature list describes the two prescriptions, the Tone control and the measurement tool. The release workflow is unchanged.
