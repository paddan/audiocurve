use std::path::Path;

use audiocurve_app::screenshots::render_all;

/// The width of a PNG, from its IHDR chunk.
fn png_width(path: &Path) -> u32 {
    let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    assert_eq!(
        &bytes[..8],
        b"\x89PNG\r\n\x1a\n",
        "{} is no PNG",
        path.display()
    );
    u32::from_be_bytes(bytes[16..20].try_into().unwrap())
}

#[test]
fn screenshot_example_writes_ten_images() {
    let dir = tempfile::tempdir().unwrap();
    render_all(dir.path());
    let names = [
        "audiogram",
        "gain",
        "spectrum",
        "hearing-test-running",
        "hearing-test-result",
    ];
    for name in names {
        for theme in ["light", "dark"] {
            let path = dir.path().join(format!("{name}-{theme}.png"));
            assert_eq!(png_width(&path), 1800, "{}", path.display());
        }
    }
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 10);
}
