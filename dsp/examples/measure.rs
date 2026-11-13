//! Offline sound-quality measurement. Writes `measurements/report.{md,json}`; with `--wav`
//! also input/output WAV pairs per metric to `measurements/wav/`.
//! usage: cargo run --release --example measure [--wav]
#[path = "../tests/support/measure/mod.rs"]
mod measure;

use std::path::Path;

fn main() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("measurements");
    let wav = std::env::args().any(|a| a == "--wav").then(|| dir.join("wav"));
    let map = measure::run_all(wav.as_deref());
    measure::report::write(&map, &dir);
    println!("wrote {}", dir.join("report.md").display());
}
