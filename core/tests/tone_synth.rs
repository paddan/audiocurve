use audiocurve_core::profile::Ear;
use audiocurve_core::tone::ToneSynth;

const FS: f64 = 48_000.0;

struct Out {
    left: Vec<f32>,
    right: Vec<f32>,
    finished: bool,
}

/// Renders `seconds` of tone in blocks of 512 frames.
fn render_all(ear: Ear, f: f64, level: Option<f64>, seconds: f64) -> Out {
    let mut synth = ToneSynth::new(FS);
    synth.start(ear, f, level);
    let total = (seconds * FS) as usize;
    let (mut l, mut r) = (vec![1.0f32; total], vec![1.0f32; total]);
    let mut pos = 0;
    while pos < total {
        let n = 512.min(total - pos);
        synth.render(&mut l[pos..pos + n], &mut r[pos..pos + n]);
        pos += n;
    }
    Out { left: l, right: r, finished: synth.is_finished() }
}

fn tone(ear: Ear, f: f64, level: Option<f64>) -> Out {
    render_all(ear, f, level, 1.1)
}

fn peak(x: &[f32]) -> f32 {
    x.iter().fold(0.0, |m, v| m.max(v.abs()))
}

fn range(a: f64, b: f64) -> std::ops::Range<usize> {
    (a * FS) as usize..(b * FS) as usize
}

#[test]
fn clamps_to_cap_even_if_asked_louder() {
    let out = tone(Ear::Left, 4000.0, Some(0.0));
    let cap = (10f64.powf(-5.0 / 20.0) + 1e-6) as f32;
    assert!(peak(&out.left) <= cap);
    assert!(peak(&out.left) > 0.5);
}

#[test]
fn infinite_level_is_silent_and_finite() {
    for level in [f64::INFINITY, f64::NEG_INFINITY, f64::NAN] {
        let out = tone(Ear::Left, 1000.0, Some(level));
        assert!(out.left.iter().all(|v| v.is_finite()));
        assert_eq!(peak(&out.left), 0.0);
        assert_eq!(peak(&out.right), 0.0);
    }
}

#[test]
fn over_cap_request_is_a_scaled_sine_not_clipped() {
    for level in [0.0, 20.0, 200.0] {
        let out = tone(Ear::Left, 4000.0, Some(level));
        let cap = 10f64.powf(-5.0 / 20.0) as f32;
        let plateau = &out.left[range(0.05, 0.15)];
        assert!(peak(plateau) <= cap + 1e-6);
        assert!(peak(plateau) > cap * 0.99);
        let rms = (plateau.iter().map(|&v| f64::from(v) * f64::from(v)).sum::<f64>() / plateau.len() as f64).sqrt();
        assert!((rms / f64::from(peak(plateau)) - 0.5f64.sqrt()).abs() < 0.005);
    }
}

#[test]
fn caps_per_frequency() {
    for (f, cap_db) in [(250.0, -1.0), (1000.0, -3.0), (4000.0, -5.0)] {
        let out = tone(Ear::Right, f, Some(0.0));
        let cap = 10f64.powf(cap_db / 20.0) as f32;
        let p = peak(&out.right);
        assert!(p <= cap + 1e-6);
        assert!(p > cap * 0.99);
    }
}

#[test]
fn level_matches_request() {
    let out = tone(Ear::Left, 1000.0, Some(-40.0));
    let p = f64::from(peak(&out.left[range(0.05, 0.15)]));
    assert!((20.0 * p.log10() + 40.0).abs() < 0.1);
}

#[test]
fn only_chosen_ear() {
    let out = tone(Ear::Left, 1000.0, Some(-20.0));
    assert_eq!(peak(&out.right), 0.0);
    assert!(peak(&out.left) > 0.0);
    let out2 = tone(Ear::Right, 1000.0, Some(-20.0));
    assert_eq!(peak(&out2.left), 0.0);
    assert!(peak(&out2.right) > 0.0);
}

#[test]
fn ramps_start_and_end_at_zero() {
    let out = tone(Ear::Left, 1000.0, Some(-20.0));
    assert!(out.left[0].abs() < 1e-4);
    assert!(out.left[(0.2 * FS) as usize - 1].abs() < 1e-3);
    let early = peak(&out.left[0..(0.002 * FS) as usize]);
    let plateau = peak(&out.left[range(0.05, 0.15)]);
    assert!(early < plateau * 0.1);
    assert!(peak(&out.left[range(0.018, 0.022)]) > plateau * 0.9);
}

#[test]
fn three_pulses_then_silence() {
    let out = tone(Ear::Left, 1000.0, Some(-20.0));
    for (a, b) in [(0.0, 0.2), (0.4, 0.6), (0.8, 1.0)] {
        assert!(peak(&out.left[range(a + 0.03, b - 0.03)]) > 0.05);
    }
    for (a, b) in [(0.2, 0.4), (0.6, 0.8), (1.0, 1.1)] {
        assert!(peak(&out.left[range(a, b)]) < 1e-6);
    }
    assert!(out.finished);
}

#[test]
fn catch_trial_is_silent() {
    let out = tone(Ear::Right, 1000.0, None);
    assert_eq!(peak(&out.left), 0.0);
    assert_eq!(peak(&out.right), 0.0);
    assert!(out.finished);
    let mut synth = ToneSynth::new(FS);
    synth.start(Ear::Right, 1000.0, None);
    let (mut l, mut r) = (vec![0.0f32; 24_000], vec![0.0f32; 24_000]);
    synth.render(&mut l, &mut r);
    assert!(!synth.is_finished());
}
