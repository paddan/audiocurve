//! Draws the egui app's screenshots: `cargo run --example screenshots -- <out dir>`.

use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let Some(dir) = std::env::args_os().nth(1).map(PathBuf::from) else {
        eprintln!("usage: screenshots <out dir>");
        return ExitCode::FAILURE;
    };
    audiocurve_app::screenshots::render_all(&dir);
    println!("wrote 10 screenshots to {}", dir.display());
    ExitCode::SUCCESS
}
