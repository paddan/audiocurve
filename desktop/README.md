# AudioCurve egui app

The AudioCurve macOS app in Rust and egui. `make release` in the repository root builds it into
`AudioCurve.app`, next to the Swift AUv3 extension (`AU/`).

## Stage 1

Stage 1 is the whole app above the platform: the model (`src/model/`), the egui UI (`src/ui/`)
and the engine wrapper, all running on a **fake platform** (`src/platform/fake.rs`). The fake
platform has three demo outputs (AirPods Pro, Hearing Aids, Mac Studio Speakers) and a route
that is fed pink noise in real time, so the meters, gain and spectrum move. No sound reaches a
real device, and there is no tray or Dock icon yet.

Profiles and settings are read from and written to the standard profile store
(`ProfileStore::standard()`), the same place the Swift app uses.

Left to stage 2: the CoreAudio/AppKit backend behind the traits in `src/platform/mod.rs`, the
real tray, the Dock icon and reopen handling, launch at login, keeping the app running in
background mode when the window closes (stage 1 always quits), the migration of the Swift app's
settings, and packaging.

## Run

```sh
cd desktop
cargo run
```

The window follows the system's light or dark appearance and language.

## Test

```sh
cd desktop
cargo test
cargo clippy --all-targets -- -D warnings
```

The UI tests draw with `egui_kittest` on wgpu, so they need a GPU (any Mac has one).

## Screenshots

```sh
make screenshots
```

This draws the audiogram, gain, spectrum, hearing-test-running and hearing-test-result
screens, light and dark, as 1800 px wide PNGs (900 × 720 at 2×), in English, with a made-up
example audiogram and a temporary profile store, and copies them into `docs/screenshots/` under
the main README's names. To write them elsewhere: `cargo run --example screenshots -- <out dir>`.
