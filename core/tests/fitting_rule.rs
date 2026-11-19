mod support;

use audiocurve_core::fitting::{self, CompressionMode, ParamsInput, Prescription};
use audiocurve_core::profile::{Audiogram, AudiogramPoint, Ear, HearingProfile};
use audiocurve_core::{BAND_COUNT, CENTERS};
use audiocurve_dsp::BandFitting;
use support::example_profile;

const RXS: [Prescription; 2] = [Prescription::Speech, Prescription::Music];

fn flat_audiogram(hl: f64) -> Audiogram {
    Audiogram {
        points: Audiogram::STANDARD_FREQUENCIES
            .iter()
            .map(|&f| AudiogramPoint { frequency_hz: f, threshold_db_hl: hl })
            .collect(),
    }
}

fn uniform_profile(hl: f64) -> HearingProfile {
    HearingProfile { left: flat_audiogram(hl), right: flat_audiogram(hl), left_trim_db: vec![0.0; 12], right_trim_db: vec![0.0; 12] }
}

fn bands(p: &HearingProfile, ear: Ear, s: f64, rx: Prescription) -> [BandFitting; BAND_COUNT] {
    fitting::bands(p, ear, s, 0.0, rx)
}

fn expect_fitting(b: &BandFitting, soft: f64, loud: f64, cr: f64) {
    assert!((b.gain_soft_db - soft).abs() < 1e-9, "soft {} != {soft}", b.gain_soft_db);
    assert!((b.gain_loud_db - loud).abs() < 1e-9, "loud {} != {loud}", b.gain_loud_db);
    assert!((b.compression_ratio - cr).abs() < 1e-9, "cr {} != {cr}", b.compression_ratio);
}

fn hls() -> impl Iterator<Item = f64> {
    (0..=12).map(|i| f64::from(i) * 10.0)
}

#[test]
fn prescription_cases_match_fixture() {
    let rows: Vec<serde_json::Value> = serde_json::from_str(include_str!(
        "../../Packages/AudioCurveCore/Tests/AudioCurveCoreTests/Fixtures/prescription-cases.json"
    ))
    .unwrap();
    assert!(rows.len() >= 12);
    for r in &rows {
        let n = |k: &str| r[k].as_f64().unwrap();
        let rx = if r["prescription"] == "speech" { Prescription::Speech } else { Prescription::Music };
        let b = fitting::bands(&uniform_profile(n("hl")), Ear::Left, 1.0, n("tone"), rx);
        let i = CENTERS.iter().position(|&c| c == n("f")).unwrap();
        assert!((b[i].gain_soft_db - n("soft")).abs() < 0.001, "soft {r}");
        assert!((b[i].gain_loud_db - n("loud")).abs() < 0.001, "loud {r}");
        assert!((b[i].compression_ratio - n("cr")).abs() < 0.001, "cr {r}");
    }
}

#[test]
fn normal_hearing_is_unity() {
    for rx in RXS {
        for b in bands(&HearingProfile::flat(), Ear::Left, 1.0, rx) {
            expect_fitting(&b, 0.0, 0.0, 1.0);
        }
    }
}

#[test]
fn speech_ratio_at_most3() {
    for hl in hls() {
        for b in bands(&uniform_profile(hl), Ear::Left, 1.0, Prescription::Speech) {
            assert!(b.compression_ratio >= 1.0 && b.compression_ratio <= 3.0);
        }
    }
}

#[test]
fn music_ratio_at_most2_and_no_bass_cut() {
    for hl in hls() {
        let music = bands(&uniform_profile(hl), Ear::Left, 1.0, Prescription::Music);
        let hle = if hl <= 60.0 { hl } else { 60.0 + 0.5 * (hl - 60.0) };
        for i in 0..BAND_COUNT {
            assert!(music[i].compression_ratio >= 1.0 && music[i].compression_ratio <= 2.0);
            if CENTERS[i] <= 500.0 {
                assert!((music[i].gain_soft_db - 0.45 * hle).abs() < 1e-9);
                assert!((music[i].gain_loud_db - 0.3 * hle).abs() < 1e-9);
            }
        }
    }
}

#[test]
fn tone_zero_is_base() {
    let p = example_profile();
    for rx in RXS {
        assert_eq!(fitting::bands(&p, Ear::Left, 1.0, 0.0, rx), bands(&p, Ear::Left, 1.0, rx));
    }
}

#[test]
fn tone_keeps_soft_treble_and_lowers_loud_by6() {
    let p = uniform_profile(60.0);
    for rx in RXS {
        let base = fitting::bands(&p, Ear::Left, 1.0, 0.0, rx);
        let full = fitting::bands(&p, Ear::Left, 1.0, 1.0, rx);
        for i in (0..BAND_COUNT).filter(|&i| CENTERS[i] > 2000.0) {
            assert_eq!(full[i].gain_soft_db, base[i].gain_soft_db);
            assert!((full[i].gain_loud_db - (base[i].gain_loud_db - 6.0)).abs() < 1e-9);
        }
        for i in (0..BAND_COUNT).filter(|&i| CENTERS[i] <= 250.0) {
            assert!((full[i].gain_soft_db - (base[i].gain_soft_db + 5.0)).abs() < 1e-9);
            assert!((full[i].gain_loud_db - (base[i].gain_loud_db + 5.0)).abs() < 1e-9);
        }
    }
}

#[test]
fn strength_zero_tone_full_is_finite_and_non_negative() {
    for hl in hls() {
        for rx in RXS {
            for s in [0.0, 0.5, 1.0] {
                let max = if rx == Prescription::Speech { 3.0 } else { 2.0 };
                for b in fitting::bands(&uniform_profile(hl), Ear::Left, s, 1.0, rx) {
                    assert!(b.gain_soft_db.is_finite() && b.gain_soft_db >= 0.0);
                    assert!(b.gain_loud_db.is_finite() && b.gain_loud_db >= 0.0);
                    assert!(b.compression_ratio.is_finite() && b.compression_ratio >= 1.0 && b.compression_ratio <= max);
                }
            }
        }
    }
}

#[test]
fn example12k_holds_at8k() {
    let p = example_profile();
    for ear in Ear::ALL {
        for rx in RXS {
            let b = bands(&p, ear, 1.0, rx);
            assert_eq!(b[11], b[10]);
        }
    }
}

#[test]
fn example_left_at1500() {
    let b = bands(&example_profile(), Ear::Left, 1.0, Prescription::Speech)[5];
    expect_fitting(&b, 37.5, 15.625, 60.0 / (60.0 - 21.875));
    let m = bands(&example_profile(), Ear::Left, 1.0, Prescription::Music)[5];
    expect_fitting(&m, 0.45 * 62.5, 0.3 * 62.5, 60.0 / (60.0 - 0.15 * 62.5));
}

#[test]
fn strength_scales_and_zero_is_unity() {
    let p = example_profile();
    for b in bands(&p, Ear::Left, 0.0, Prescription::Speech) {
        expect_fitting(&b, 0.0, 0.0, 1.0);
    }
    let full = bands(&p, Ear::Left, 1.0, Prescription::Speech);
    let half = bands(&p, Ear::Left, 0.5, Prescription::Speech);
    assert!((half[9].gain_soft_db - full[9].gain_soft_db / 2.0).abs() < 1e-9);
    assert!((half[9].gain_loud_db - full[9].gain_loud_db / 2.0).abs() < 1e-9);
}

#[test]
fn nan_strength_is_zero() {
    for b in bands(&example_profile(), Ear::Left, f64::NAN, Prescription::Speech) {
        expect_fitting(&b, 0.0, 0.0, 1.0);
    }
}

#[test]
fn short_trim_array_does_not_crash() {
    let mut p = example_profile();
    p.left_trim_db = vec![];
    assert_eq!(
        bands(&p, Ear::Left, 1.0, Prescription::Speech),
        bands(&example_profile(), Ear::Left, 1.0, Prescription::Speech)
    );
}

#[test]
fn trim_adds_to_both() {
    let mut p = example_profile();
    let mut t = p.trim(Ear::Left).to_vec();
    t[4] = 5.0;
    p.set_trim(Ear::Left, t);
    let base = bands(&example_profile(), Ear::Left, 1.0, Prescription::Speech);
    let left = bands(&p, Ear::Left, 1.0, Prescription::Speech);
    assert!((left[4].gain_soft_db - (base[4].gain_soft_db + 5.0)).abs() < 1e-9);
    assert!((left[4].gain_loud_db - (base[4].gain_loud_db + 5.0)).abs() < 1e-9);
    assert_eq!(
        bands(&p, Ear::Right, 1.0, Prescription::Speech),
        bands(&example_profile(), Ear::Right, 1.0, Prescription::Speech)
    );
}

#[test]
fn blend_is_linear_in_gains() {
    let sp = bands(&example_profile(), Ear::Left, 1.0, Prescription::Speech);
    let mu = bands(&example_profile(), Ear::Left, 1.0, Prescription::Music);
    assert_eq!(fitting::blend(&sp, &mu, 1.0), sp);
    assert_eq!(fitting::blend(&sp, &mu, 0.0), mu);
    let h = fitting::blend(&sp, &mu, 0.5);
    for i in 0..BAND_COUNT {
        assert!((h[i].gain_soft_db - (sp[i].gain_soft_db + mu[i].gain_soft_db) / 2.0).abs() < 1e-9);
        assert!((h[i].gain_loud_db - (sp[i].gain_loud_db + mu[i].gain_loud_db) / 2.0).abs() < 1e-9);
    }
}

#[test]
fn interpolation_log_frequency() {
    let a = support::audiogram_from(&[(1000.0, 20.0), (2000.0, 60.0), (4000.0, 60.0), (8000.0, 60.0)]);
    assert!((fitting::interpolated_hl(&a, 1500.0) - 43.40).abs() < 0.01);
}

#[test]
fn gain_curve_matches_rust() {
    let b = BandFitting { gain_soft_db: 60.0, gain_loud_db: 25.0, compression_ratio: 4.5 };
    for (l, want) in [(-90.0, 45.0), (-70.0, 60.0), (-60.0, 60.0), (-15.0, 25.0)] {
        assert!((fitting::gain_db(&b, l) - want).abs() < 1e-9);
        assert_eq!(fitting::gain_db(&b, l), audiocurve_dsp::gain_db(&b, l));
    }
}

#[test]
fn params_packing() {
    let profile = example_profile();
    let mut input = ParamsInput::new(Some(&profile), 1.0, -3.0, -6.0, CompressionMode::Music);
    input.tone = 0.5;
    let p = fitting::params(&input);
    for (ear, speech, music) in [(Ear::Left, p.left, p.music_left), (Ear::Right, p.right, p.music_right)] {
        assert_eq!(speech, fitting::bands(&profile, ear, 1.0, 0.5, Prescription::Speech));
        assert_eq!(music, fitting::bands(&profile, ear, 1.0, 0.5, Prescription::Music));
    }
    assert!((p.mpo_dbfs - -3.0).abs() < 1e-9);
    assert!((p.output_gain_db - -6.0).abs() < 1e-9);
    assert_eq!(p.speech_weight, 0.0);
}

#[test]
fn nil_profile_is_unity() {
    let p = fitting::params(&ParamsInput::new(None, 1.0, -3.0, 0.0, CompressionMode::Speech));
    for b in p.left.iter().chain(p.right.iter()) {
        assert!(b.gain_soft_db == 0.0 && b.gain_loud_db == 0.0 && b.compression_ratio == 1.0);
    }
    assert_eq!(p.speech_weight, 1.0);
}

#[test]
fn host_parameters_are_clamped() {
    let p = |mpo: f64, gain: f64| fitting::params(&ParamsInput::new(None, 0.5, mpo, gain, CompressionMode::Speech));
    assert_eq!(p(5.0, 100.0).mpo_dbfs, -1.0);
    assert_eq!(p(5.0, 100.0).output_gain_db, 12.0);
    assert_eq!(p(-100.0, -100.0).mpo_dbfs, -20.0);
    assert_eq!(p(-100.0, -100.0).output_gain_db, -48.0);
    assert_eq!(p(-5.0, 3.0).mpo_dbfs, -5.0);
    assert_eq!(p(-5.0, 3.0).output_gain_db, 3.0);
    assert_eq!(p(f64::NAN, f64::NAN).mpo_dbfs, -3.0);
    assert_eq!(p(f64::NAN, f64::NAN).output_gain_db, 0.0);
    assert_eq!(p(f64::INFINITY, f64::NEG_INFINITY).mpo_dbfs, -1.0);
    assert_eq!(p(f64::INFINITY, f64::NEG_INFINITY).output_gain_db, -48.0);
}

#[test]
fn compression_off_gives_fixed_loud_gain() {
    let profile = example_profile();
    let mut input = ParamsInput::new(Some(&profile), 0.5, -3.0, 0.0, CompressionMode::Speech);
    input.compression = false;
    let p = fitting::params(&input);
    let expected = bands(&profile, Ear::Left, 0.5, Prescription::Speech);
    let expected_music = bands(&profile, Ear::Left, 0.5, Prescription::Music);
    for (set, want) in [(p.left, expected), (p.music_left, expected_music)] {
        for i in 0..BAND_COUNT {
            assert_eq!(set[i].compression_ratio, 1.0);
            assert_eq!(set[i].gain_soft_db, want[i].gain_loud_db);
            assert_eq!(set[i].gain_loud_db, want[i].gain_loud_db);
        }
    }
    assert_eq!(p.limiter_enabled, 1.0);
}

#[test]
fn limiter_flag_passes_through() {
    let mut input = ParamsInput::new(None, 0.5, -3.0, 0.0, CompressionMode::Music);
    input.limiter = false;
    assert_eq!(fitting::params(&input).limiter_enabled, 0.0);
}

#[test]
fn input_gain_passes_through_clamped() {
    let with = |g: f64| {
        let mut input = ParamsInput::new(None, 0.5, -3.0, 0.0, CompressionMode::Music);
        input.input_gain_db = g;
        fitting::params(&input).input_gain_db
    };
    assert_eq!(with(-12.0), -12.0);
    assert_eq!(with(-99.0), -30.0);
    assert_eq!(with(f64::NAN), 0.0);
    assert_eq!(fitting::params(&ParamsInput::new(None, 0.5, -3.0, 0.0, CompressionMode::Music)).input_gain_db, 0.0);
}

#[test]
fn auto_mode_sets_flag_with_zero_speech_weight() {
    let a = fitting::params(&ParamsInput::new(None, 0.5, -3.0, 0.0, CompressionMode::Auto));
    assert_eq!(a.auto_mode, 1.0);
    assert_eq!(a.speech_weight, 0.0);
    let s = fitting::params(&ParamsInput::new(None, 0.5, -3.0, 0.0, CompressionMode::Speech));
    assert_eq!(s.auto_mode, 0.0);
    assert_eq!(s.speech_weight, 1.0);
}

#[test]
fn params_carry_active_flag() {
    let on = fitting::params(&ParamsInput::new(None, 0.5, -3.0, 0.0, CompressionMode::Music));
    assert_eq!(on.active, 1.0);
    let mut input = ParamsInput::new(None, 0.5, -3.0, 0.0, CompressionMode::Music);
    input.active = false;
    assert_eq!(fitting::params(&input).active, 0.0);
}
