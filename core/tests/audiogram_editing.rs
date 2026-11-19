mod support;

use audiocurve_core::editing::{self, PointSelection};
use audiocurve_core::profile::{Audiogram, Ear, HearingProfile};
use support::{audiogram_from, example_profile};

fn sel(ear: Ear, f: f64) -> PointSelection {
    PointSelection { ear, frequency_hz: f }
}

fn threshold(p: &HearingProfile, ear: Ear, f: f64) -> Option<f64> {
    p.audiogram(ear).points.iter().find(|q| q.frequency_hz == f).map(|q| q.threshold_db_hl)
}

#[test]
fn snap_coarse_and_fine() {
    assert_eq!(editing::snap(83.0, false), 85.0);
    assert_eq!(editing::snap(83.0, true), 83.0);
    assert_eq!(editing::snap(130.0, false), 120.0);
    assert_eq!(editing::snap(-14.0, false), -10.0);
}

#[test]
fn set_value_adds_missing_point() {
    let mut p = example_profile();
    p.left.points.retain(|q| q.frequency_hz != 750.0);
    editing::set_value(&mut p, sel(Ear::Left, 750.0), 40.0, false);
    assert_eq!(threshold(&p, Ear::Left, 750.0), Some(40.0));
    let freqs: Vec<f64> = p.left.points.iter().map(|q| q.frequency_hz).collect();
    assert!(freqs.windows(2).all(|w| w[0] < w[1]));
    assert_eq!(freqs.len(), 11);
}

#[test]
fn set_value_ignores_non_standard_frequency() {
    let mut p = example_profile();
    let before = p.clone();
    editing::set_value(&mut p, sel(Ear::Left, 900.0), 40.0, false);
    assert_eq!(p, before);
}

#[test]
fn nudge_up_lowers_hl() {
    let mut coarse = example_profile();
    editing::nudge(&mut coarse, sel(Ear::Left, 1500.0), true, false);
    assert_eq!(threshold(&coarse, Ear::Left, 1500.0), Some(60.0));
    let mut fine = example_profile();
    editing::nudge(&mut fine, sel(Ear::Left, 1500.0), true, true);
    assert_eq!(threshold(&fine, Ear::Left, 1500.0), Some(64.0));
}

#[test]
fn nudge_down_raises_hl() {
    let mut p = example_profile();
    editing::nudge(&mut p, sel(Ear::Left, 1500.0), false, false);
    assert_eq!(threshold(&p, Ear::Left, 1500.0), Some(70.0));
}

#[test]
fn nudge_clamps() {
    let mut p = HearingProfile::flat();
    for q in &mut p.left.points {
        q.threshold_db_hl = -10.0;
    }
    editing::nudge(&mut p, sel(Ear::Left, 1000.0), true, false);
    assert_eq!(threshold(&p, Ear::Left, 1000.0), Some(-10.0));
}

#[test]
fn neighbor_skips_missing() {
    let mut p = HearingProfile::flat();
    p.left = audiogram_from(&[(500.0, 10.0), (1000.0, 10.0), (2000.0, 10.0)]);
    assert_eq!(editing::neighbor(&p, sel(Ear::Left, 500.0), true), Some(sel(Ear::Left, 1000.0)));
    assert_eq!(editing::neighbor(&p, sel(Ear::Left, 2000.0), true), None);
    assert_eq!(editing::neighbor(&p, sel(Ear::Left, 2000.0), false), Some(sel(Ear::Left, 1000.0)));
    assert_eq!(editing::neighbor(&p, sel(Ear::Left, 500.0), false), None);
}

#[test]
fn switch_ear_same_or_nearest() {
    let mut p = HearingProfile::flat();
    p.left = audiogram_from(&[(1000.0, 10.0), (1500.0, 10.0)]);
    p.right = audiogram_from(&[(1000.0, 20.0), (2000.0, 20.0)]);
    assert_eq!(editing::switch_ear(&p, sel(Ear::Left, 1000.0)), sel(Ear::Right, 1000.0));
    assert_eq!(editing::switch_ear(&p, sel(Ear::Left, 1500.0)), sel(Ear::Right, 2000.0));
    assert_eq!(editing::switch_ear(&p, sel(Ear::Right, 2000.0)), sel(Ear::Left, 1500.0));
}

#[test]
fn switch_ear_keeps_selection_when_other_ear_empty() {
    let mut p = HearingProfile::flat();
    p.right = Audiogram { points: vec![] };
    assert_eq!(editing::switch_ear(&p, sel(Ear::Left, 1000.0)), sel(Ear::Left, 1000.0));
}

#[test]
fn delete_respects_minimum() {
    let mut four = HearingProfile::flat();
    four.left = audiogram_from(&[(500.0, 10.0), (1000.0, 10.0), (2000.0, 10.0), (4000.0, 10.0)]);
    let before = four.clone();
    assert!(!editing::delete(&mut four, sel(Ear::Left, 1000.0)));
    assert_eq!(four, before);

    let mut five = HearingProfile::flat();
    five.left = audiogram_from(&[(500.0, 10.0), (1000.0, 10.0), (2000.0, 10.0), (4000.0, 10.0), (6000.0, 10.0)]);
    assert!(editing::delete(&mut five, sel(Ear::Left, 1000.0)));
    let freqs: Vec<f64> = five.left.points.iter().map(|q| q.frequency_hz).collect();
    assert_eq!(freqs, [500.0, 2000.0, 4000.0, 6000.0]);
}

#[test]
fn parse_typed() {
    assert_eq!(editing::parse_typed("85"), Some(85.0));
    assert_eq!(editing::parse_typed(" 85 "), Some(85.0));
    assert_eq!(editing::parse_typed("-5"), Some(-5.0));
    assert_eq!(editing::parse_typed("\u{2212}5"), Some(-5.0));
    assert_eq!(editing::parse_typed("130"), None);
    assert_eq!(editing::parse_typed("-11"), None);
    assert_eq!(editing::parse_typed("abc"), None);
    assert_eq!(editing::parse_typed(""), None);
}

#[test]
fn y_mapping() {
    assert_eq!(editing::db_hl_at_y(0.0, 130.0), -10.0);
    assert_eq!(editing::db_hl_at_y(130.0, 130.0), 120.0);
    assert_eq!(editing::db_hl_at_y(65.0, 130.0), 55.0);
}

#[test]
fn trim_clamp_and_reset() {
    let mut p = HearingProfile::flat();
    editing::nudge_trim(&mut p, Ear::Left, 3, 25.0);
    assert_eq!(p.left_trim_db[3], 20.0);
    assert_eq!(p.right_trim_db[3], 0.0);
    editing::nudge_trim(&mut p, Ear::Left, 3, -45.0);
    assert_eq!(p.left_trim_db[3], -20.0);
    editing::reset_trim(&mut p, Ear::Left, 3);
    assert_eq!(p.left_trim_db[3], 0.0);
}
