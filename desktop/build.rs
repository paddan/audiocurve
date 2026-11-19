//! Generates `OUT_DIR/strings.rs`: a `match`-based lookup from the xcstrings catalog.

use std::fmt::Write as _;
use std::path::PathBuf;

const LANGS: [&str; 7] = ["sv", "da", "nb", "fi", "de", "fr", "es"];

fn main() {
    println!("cargo:rerun-if-changed=strings/Localizable.xcstrings");
    println!("cargo:rerun-if-changed=build.rs");
    let text = std::fs::read_to_string("strings/Localizable.xcstrings").expect("read catalog");
    let catalog: serde_json::Value = serde_json::from_str(&text).expect("parse catalog");
    let strings = catalog["strings"].as_object().expect("strings object");

    let mut out = String::from(
        "fn lookup(lang: Lang, key: &str) -> Option<&'static str> {\n    match lang {\n        Lang::En => None,\n",
    );
    for code in LANGS {
        let variant = match code {
            "sv" => "Sv",
            "da" => "Da",
            "nb" => "Nb",
            "fi" => "Fi",
            "de" => "De",
            "fr" => "Fr",
            _ => "Es",
        };
        let _ = writeln!(out, "        Lang::{variant} => match key {{");
        for (key, entry) in strings {
            if let Some(v) = entry["localizations"][code]["stringUnit"]["value"].as_str() {
                let _ = writeln!(out, "            {key:?} => Some({v:?}),");
            }
        }
        out.push_str("            _ => None,\n        },\n");
    }
    out.push_str("    }\n}\n");
    let dest = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("strings.rs");
    std::fs::write(dest, out).expect("write strings.rs");
}
