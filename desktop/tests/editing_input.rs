mod support;

use std::time::Instant;

use audiocurve_app::i18n::{self, Lang};
use audiocurve_app::model::editing_input::Key;
use audiocurve_app::model::editor::{Editor, Pane, TrimSelection};
use audiocurve_app::ui::geometry::{GainGeometry, PlotGeometry};
use audiocurve_core::editing::{self, PointSelection};
use audiocurve_core::profile::{Ear, HearingProfile};
use audiocurve_core::store::ProfileStore;
use support::example_profile;

const W: f64 = 640.0;
const H: f64 = 300.0;

fn editor() -> (tempfile::TempDir, Editor) {
    i18n::set_lang(Lang::En);
    let dir = tempfile::tempdir().unwrap();
    let mut e = Editor::new(ProfileStore::new(dir.path().to_path_buf()));
    e.set_profile(example_profile(), Instant::now());
    (dir, e)
}

fn sel(ear: Ear, f: f64) -> Option<PointSelection> {
    Some(PointSelection {
        ear,
        frequency_hz: f,
    })
}

fn threshold(p: &HearingProfile, ear: Ear, f: f64) -> Option<f64> {
    p.audiogram(ear)
        .points
        .iter()
        .find(|q| q.frequency_hz == f)
        .map(|q| q.threshold_db_hl)
}

fn key(e: &mut Editor, k: Key, fine: bool) -> bool {
    e.handle_key(k, fine, Instant::now())
}

fn remove_point(e: &mut Editor, ear: Ear, f: f64) {
    let mut p = e.profile.clone().unwrap();
    let mut a = p.audiogram(ear).clone();
    a.points.retain(|q| q.frequency_hz != f);
    p.set_audiogram(ear, a);
    e.set_profile(p, Instant::now());
}

#[test]
fn arrow_up_lowers_threshold_by_5_and_option_by_1() {
    let (_d, mut e) = editor();
    e.selection = sel(Ear::Left, 1000.0);
    assert!(key(&mut e, Key::Up, false));
    assert_eq!(
        threshold(e.profile.as_ref().unwrap(), Ear::Left, 1000.0),
        Some(50.0)
    );
    assert!(key(&mut e, Key::Up, true));
    assert_eq!(
        threshold(e.profile.as_ref().unwrap(), Ear::Left, 1000.0),
        Some(49.0)
    );
    assert!(key(&mut e, Key::Down, false));
    assert_eq!(
        threshold(e.profile.as_ref().unwrap(), Ear::Left, 1000.0),
        Some(55.0),
        "a coarse step snaps to 5 dB"
    );
    assert!(
        e.next_deadline().is_some(),
        "edits go through set_profile and schedule an autosave"
    );
}

#[test]
fn left_right_move_between_existing_points() {
    let (_d, mut e) = editor();
    remove_point(&mut e, Ear::Left, 750.0);
    e.selection = sel(Ear::Left, 500.0);
    assert!(key(&mut e, Key::Right, false));
    assert_eq!(e.selection, sel(Ear::Left, 1000.0));
    assert!(key(&mut e, Key::Left, false));
    assert_eq!(e.selection, sel(Ear::Left, 500.0));
    // Without a selection Right picks the first point and Left the last.
    e.selection = None;
    key(&mut e, Key::Right, false);
    assert_eq!(e.selection, sel(Ear::Left, 125.0));
    e.selection = None;
    key(&mut e, Key::Left, false);
    assert_eq!(e.selection, sel(Ear::Left, 8000.0));
}

#[test]
fn tab_switches_ear_keeping_frequency_or_nearest() {
    let (_d, mut e) = editor();
    e.selection = sel(Ear::Left, 750.0);
    assert!(key(&mut e, Key::Tab, false));
    assert_eq!(e.selection, sel(Ear::Right, 750.0));
    assert_eq!(e.active_ear, Ear::Right);
    // 750 is missing on the left: 1000 is nearer in log-frequency than 500.
    remove_point(&mut e, Ear::Left, 750.0);
    assert!(key(&mut e, Key::Tab, false));
    assert_eq!(e.selection, sel(Ear::Left, 1000.0));
    // No selection: switch the active ear and select its first point.
    e.selection = None;
    key(&mut e, Key::Tab, false);
    assert_eq!(e.active_ear, Ear::Right);
    assert_eq!(e.selection, sel(Ear::Right, 125.0));
}

#[test]
fn delete_removes_point_and_selects_neighbor() {
    let (_d, mut e) = editor();
    e.selection = sel(Ear::Left, 1000.0);
    assert!(key(&mut e, Key::Delete, false));
    assert_eq!(
        threshold(e.profile.as_ref().unwrap(), Ear::Left, 1000.0),
        None
    );
    assert_eq!(e.selection, sel(Ear::Left, 750.0));
    // The first point has no previous neighbour: the next one is selected.
    e.selection = sel(Ear::Left, 125.0);
    key(&mut e, Key::Delete, false);
    assert_eq!(e.selection, sel(Ear::Left, 250.0));
}

#[test]
fn delete_keeps_the_minimum_number_of_points_and_says_so() {
    let (_d, mut e) = editor();
    for f in [250.0, 500.0, 750.0, 1000.0, 1500.0, 2000.0, 3000.0] {
        remove_point(&mut e, Ear::Left, f);
    }
    assert_eq!(e.profile.as_ref().unwrap().left.points.len(), 4);
    e.selection = sel(Ear::Left, 4000.0);
    assert!(key(&mut e, Key::Delete, false));
    assert_eq!(e.profile.as_ref().unwrap().left.points.len(), 4);
    assert_eq!(e.selection, sel(Ear::Left, 4000.0));
    assert_eq!(e.error.as_deref(), Some("At least 4 points per ear"));
}

#[test]
fn typing_digits_then_return_sets_value() {
    let (_d, mut e) = editor();
    e.selection = sel(Ear::Left, 1000.0);
    assert!(key(&mut e, Key::Char('4'), false));
    assert!(key(&mut e, Key::Char('5'), false));
    assert_eq!(e.typed_buffer, "45");
    assert_eq!(e.point_label(e.selection.unwrap()), "L 1 kHz · 45_ dB HL");
    assert!(key(&mut e, Key::Return, false));
    assert_eq!(
        threshold(e.profile.as_ref().unwrap(), Ear::Left, 1000.0),
        Some(45.0)
    );
    assert!(e.typed_buffer.is_empty());
    assert_eq!(e.point_label(e.selection.unwrap()), "L 1 kHz · 45 dB HL");
}

#[test]
fn typing_supports_minus_backspace_and_four_characters() {
    let (_d, mut e) = editor();
    e.selection = sel(Ear::Left, 1000.0);
    for c in ['-', '5', '9', '9', '9'] {
        key(&mut e, Key::Char(c), false);
    }
    assert_eq!(e.typed_buffer, "-599");
    key(&mut e, Key::Delete, false);
    assert_eq!(e.typed_buffer, "-59");
    key(&mut e, Key::Escape, false);
    assert!(e.typed_buffer.is_empty());
    e.selection = sel(Ear::Left, 1000.0);
    key(&mut e, Key::Char('-'), false);
    key(&mut e, Key::Char('\u{2212}'), false);
    key(&mut e, Key::Char('5'), false);
    assert_eq!(e.typed_buffer, "-5");
    assert!(
        !key(&mut e, Key::Char('x'), false),
        "other characters are not handled"
    );
}

#[test]
fn escape_clears_selection_and_buffer() {
    let (_d, mut e) = editor();
    e.selection = sel(Ear::Left, 1000.0);
    key(&mut e, Key::Char('4'), false);
    assert!(key(&mut e, Key::Escape, false));
    assert_eq!(e.selection, None);
    assert!(e.typed_buffer.is_empty());
}

#[test]
fn out_of_range_or_garbage_typed_value_is_ignored() {
    let (_d, mut e) = editor();
    e.selection = sel(Ear::Left, 1000.0);
    let before = e.profile.clone();
    for c in ['1', '3', '0'] {
        key(&mut e, Key::Char(c), false);
    }
    assert!(key(&mut e, Key::Return, false));
    assert_eq!(e.profile, before);
    assert!(e.typed_buffer.is_empty());
    assert_eq!(
        e.error.as_deref(),
        Some("Enter a value between −10 and 120 dB HL")
    );
    // Letters are never accepted into the buffer; Return then does nothing.
    for c in ['a', 'b', 'c'] {
        assert!(!key(&mut e, Key::Char(c), false));
    }
    assert!(key(&mut e, Key::Return, false));
    assert_eq!(e.profile, before);
    assert!(e.typed_buffer.is_empty());
    // A lone minus sign is garbage too.
    key(&mut e, Key::Char('-'), false);
    key(&mut e, Key::Return, false);
    assert_eq!(e.profile, before);
    assert!(e.typed_buffer.is_empty());
}

#[test]
fn keys_are_ignored_without_profile_or_on_spectrum() {
    let (_d, mut e) = editor();
    e.pane = Pane::Spectrum;
    assert!(!key(&mut e, Key::Up, false));
    let dir = tempfile::tempdir().unwrap();
    let mut empty = Editor::new(ProfileStore::new(dir.path().to_path_buf()));
    assert!(!key(&mut empty, Key::Up, false));
}

#[test]
fn drag_on_empty_frequency_adds_point_on_active_ear() {
    let (_d, mut e) = editor();
    remove_point(&mut e, Ear::Left, 3000.0);
    e.active_ear = Ear::Left;
    let g = PlotGeometry::new(W, H);
    let (x, y) = (g.x(3000.0), g.y(90.0));
    e.begin_drag(x, y, W, H, 0.0..=40.0, Instant::now());
    let p = e.profile.clone().unwrap();
    assert_eq!(threshold(&p, Ear::Left, 3000.0), Some(90.0));
    assert_eq!(e.selection, sel(Ear::Left, 3000.0));
    assert_eq!(
        threshold(&p, Ear::Right, 3000.0),
        Some(60.0),
        "the other ear keeps its own point"
    );
    e.drag(x, g.y(70.0), W, H, false, Instant::now());
    assert_eq!(
        threshold(e.profile.as_ref().unwrap(), Ear::Left, 3000.0),
        Some(70.0)
    );
    e.end_drag();
    e.drag(x, g.y(10.0), W, H, false, Instant::now());
    assert_eq!(
        threshold(e.profile.as_ref().unwrap(), Ear::Left, 3000.0),
        Some(70.0),
        "no drag after end"
    );
}

#[test]
fn drag_on_existing_point_selects_it_and_keeps_grab_offset() {
    let (_d, mut e) = editor();
    let g = PlotGeometry::new(W, H);
    // Right ear point at 1000 Hz (35 dB), grabbed 4 px below its centre; the active ear is left.
    e.active_ear = Ear::Left;
    e.begin_drag(
        g.x(1000.0),
        g.y(35.0) + 4.0,
        W,
        H,
        0.0..=40.0,
        Instant::now(),
    );
    // Left 1000 Hz is at 55 dB, far away, so the right point is the hit.
    assert_eq!(e.selection, sel(Ear::Right, 1000.0));
    assert_eq!(e.active_ear, Ear::Right);
    // A move below the 2 px threshold does nothing.
    e.drag(g.x(1000.0), g.y(35.0) + 5.0, W, H, false, Instant::now());
    assert_eq!(
        threshold(e.profile.as_ref().unwrap(), Ear::Right, 1000.0),
        Some(35.0)
    );
    // Dragging by +40 px moves the value by the equivalent amount (offset keeps the point under the cursor).
    let y = g.y(35.0) + 4.0 + (g.y(60.0) - g.y(35.0));
    e.drag(g.x(1000.0), y, W, H, false, Instant::now());
    assert_eq!(
        threshold(e.profile.as_ref().unwrap(), Ear::Right, 1000.0),
        Some(60.0)
    );
    // Click on nothing clears the selection.
    e.end_drag();
    e.begin_drag(g.x(1000.0) + 20.0, 5.0, W, H, 0.0..=40.0, Instant::now());
    assert_eq!(e.selection, None);
}

#[test]
fn trim_drag_is_clamped_to_20() {
    let (_d, mut e) = editor();
    e.pane = Pane::Gain;
    let range = -10.0..=50.0;
    let g = GainGeometry::new(W, H, range.clone());
    let slot = g.plot.width / 12.0;
    let x = g.plot.min_x + slot * 3.5 - 3.0; // left bar of band 3
    let y = g.plot.min_y + g.plot.height / 2.0;
    e.begin_drag(x, y, W, H, range.clone(), Instant::now());
    assert_eq!(
        e.trim_selection,
        Some(TrimSelection {
            ear: Ear::Left,
            band: 3
        })
    );
    e.drag(x, y - 5000.0, W, H, false, Instant::now());
    assert_eq!(e.profile.as_ref().unwrap().left_trim_db[3], 20.0);
    e.drag(x, y + 5000.0, W, H, false, Instant::now());
    assert_eq!(e.profile.as_ref().unwrap().left_trim_db[3], -20.0);
    // 60 dB over plot.height pixels: one whole dB per height/60 px, rounded.
    e.drag(
        x,
        y - g.plot.height / 60.0 * 7.0,
        W,
        H,
        false,
        Instant::now(),
    );
    assert_eq!(e.profile.as_ref().unwrap().left_trim_db[3], 7.0);
    e.end_drag();
}

#[test]
fn trim_keys_and_double_click_reset() {
    let (_d, mut e) = editor();
    e.pane = Pane::Gain;
    e.active_ear = Ear::Right;
    assert!(key(&mut e, Key::Up, false));
    assert_eq!(
        e.trim_selection,
        Some(TrimSelection {
            ear: Ear::Right,
            band: 0
        })
    );
    key(&mut e, Key::Right, false);
    key(&mut e, Key::Up, false);
    key(&mut e, Key::Up, true);
    assert_eq!(
        e.profile.as_ref().unwrap().right_trim_db[1],
        2.0,
        "trim steps are always 1 dB"
    );
    key(&mut e, Key::Tab, false);
    assert_eq!(
        e.trim_selection,
        Some(TrimSelection {
            ear: Ear::Left,
            band: 1
        })
    );
    assert_eq!(e.active_ear, Ear::Left);
    key(&mut e, Key::Left, false);
    key(&mut e, Key::Left, false);
    assert_eq!(e.trim_selection.unwrap().band, 0);
    assert!(!key(&mut e, Key::Char('5'), false));
    assert!(key(&mut e, Key::Escape, false));
    assert_eq!(e.trim_selection, None);

    // Double click resets the bar under the pointer.
    let range = -10.0..=50.0;
    let g = GainGeometry::new(W, H, range.clone());
    let slot = g.plot.width / 12.0;
    let x = g.plot.min_x + slot * 1.5 + 3.0; // right bar of band 1
    e.reset_trim_at(x, g.plot.min_y + 10.0, W, H, range, Instant::now());
    assert_eq!(e.profile.as_ref().unwrap().right_trim_db[1], 0.0);
    assert_eq!(
        e.trim_selection,
        Some(TrimSelection {
            ear: Ear::Right,
            band: 1
        })
    );
}

#[test]
fn labels_match_the_swift_format() {
    let (_d, mut e) = editor();
    assert_eq!(
        e.point_label(PointSelection {
            ear: Ear::Right,
            frequency_hz: 1500.0
        }),
        "R 1.5 kHz · 45 dB HL"
    );
    assert_eq!(
        e.point_label(PointSelection {
            ear: Ear::Right,
            frequency_hz: 1234.0
        }),
        ""
    );
    let mut p = e.profile.clone().unwrap();
    p.left_trim_db[3] = 4.0;
    e.set_profile(p, Instant::now());
    let t = TrimSelection {
        ear: Ear::Left,
        band: 3,
    };
    let label = e.trim_label(t, 12.5);
    assert!(label.starts_with("L "), "{label}");
    assert!(label.contains("trim +4 dB · gain 12.5 dB"), "{label}");
    let mut p = e.profile.clone().unwrap();
    p.left.points[0].threshold_db_hl = -5.0;
    e.set_profile(p, Instant::now());
    assert_eq!(
        e.point_label(PointSelection {
            ear: Ear::Left,
            frequency_hz: 125.0
        }),
        "L 125 Hz · \u{2212}5 dB HL"
    );
}

#[test]
fn geometry_round_trips_frequency_and_db() {
    let g = PlotGeometry::new(W, H);
    for v in [-10.0, 0.0, 37.5, 120.0] {
        let back = editing::db_hl_at_y(g.y(v) - g.plot.min_y, g.plot.height);
        assert!((back - v).abs() < 1e-9, "{v} -> {back}");
    }
    for f in [125.0, 1000.0, 8000.0] {
        let octaves = (g.x(f) - g.plot.min_x) / g.plot.width * PlotGeometry::OCTAVES;
        assert!((125.0 * 2f64.powf(octaves) - f).abs() < 1e-6);
    }
    assert_eq!(g.hit(g.x(1500.0) + 3.0, g.y(20.0)), Some(1500.0));
    assert_eq!(g.hit(g.x(1500.0) + 25.0, g.y(20.0)), None);
    // Zero and negative sizes stay finite.
    for (w, h) in [(0.0, 0.0), (-5.0, 10.0)] {
        let d = PlotGeometry::new(w, h);
        assert!(d.x(1000.0).is_finite() && d.y(50.0).is_finite());
        let gg = GainGeometry::new(w, h, 0.0..=40.0);
        assert!(gg.y(10.0).is_finite());
        let _ = gg.hit(0.0, 0.0);
    }
}
