# audiocurve-core

The platform-independent logic of AudioCurve in Rust: prescription, hearing test, profiles and
their storage, per-output settings, and the system-wide route state machine. It is a port of the
Swift package `Packages/AudioCurveCore`, which the Swift app and Audio Unit still use; nothing in
the Swift build depends on this crate yet. It builds on the DSP kernel in `../dsp`.

Design: [docs/superpowers/specs/2026-11-18-rust-core-design.md](../docs/superpowers/specs/2026-11-18-rust-core-design.md).

```bash
cargo test
cargo clippy --all-targets -- -D warnings
```

`make test` in the repository root runs these tests together with the `dsp` and Swift tests.

## Swift → Rust

| Swift (`Sources/AudioCurveCore`) | Rust module |
|---|---|
| `Audiogram`, `Bands`, `HearingProfile` | `profile` |
| `FittingRule` | `fitting` |
| `AudiogramEditing` | `editing` |
| `TestCalibration`, `HearingTest`, `HearingTestRecord`, `TestWarning` | `hearing_test` |
| `ToneSynth` | `tone` |
| `AutoLevel` | `auto_level` |
| `ActiveOutputs`, `OutputAssignments` | `outputs` |
| `ProfileStore` | `store` (new file format; pending volumes move from UserDefaults to `pending-volume.json`) |
| `RouteDecision`, `RouteController` | `route` (driven by `tick(now)` instead of timers) |
| `VolumeSnapshot` | `volume` |
| `InstanceHeartbeat` | `heartbeat` |
| `PluginState` | `plugin_state` |

Not ported: `OutputDeviceMonitor` (CoreAudio), `PluginChannel` (AUv3 host channel), `AppMode`
(UI policy). Each Swift test has a Rust test with the same name in snake case, except the tests
of those three and the ones that only guard old Swift file formats.
