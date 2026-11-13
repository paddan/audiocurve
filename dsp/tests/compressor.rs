mod support;
use audiocurve_dsp::compressor::{BandCompressor, MUSIC_TIMING, SPEECH_TIMING, Timing, blend_timing};
use audiocurve_dsp::params::gain_db;
use audiocurve_dsp::BandFitting;
use support::*;

const FS: f64 = 48_000.0;
const TEST_TIMING: Timing = Timing { slow_attack_ms: 5.0, slow_release_ms: 50.0, fast_attack_ms: 2.0, fast_release_ms: 50.0 };

fn fitting() -> BandFitting {
    BandFitting { gain_soft_db: 40., gain_loud_db: 20., compression_ratio: 1.8 }
}

#[test]
fn steady_state_gain() {
    let b = fitting();
    for level in [-60., -40., -15.] {
        let x = sine(1000.0, level, 1.0, FS);
        let mut y = x.clone();
        let mut c = BandCompressor::new(FS);
        c.process(&mut y, &b, &TEST_TIMING, f64::INFINITY);
        let out = level_dbfs(&y, 0.5, FS);
        let expected = level + gain_db(&b, level);
        let err = (out - expected).abs();
        assert!(
            err <= 1.0,
            "level {level} dBFS: output {out:.3} dBFS, expected {expected:.3} dBFS, error {err:.3} dB"
        );
    }
}

/// Time in ms until the 10 ms-window gain (out dB − in dB) first lies within 1 dB of
/// `gain_db(b, -50)`, measured from the step down to -50 dBFS.
fn recovery_ms(release_ms: f64) -> Option<f64> {
    let b = fitting();
    let target = gain_db(&b, -50.0);
    let step = (0.5 * FS) as usize;
    let mut x = sine(1000.0, -15.0, 3.5, FS);
    let k = 10f64.powf(-35.0 / 20.0) as f32;
    for v in &mut x[step..] {
        *v *= k;
    }
    let mut y = x.clone();
    let mut c = BandCompressor::new(FS);
    let t = Timing { slow_release_ms: release_ms, ..TEST_TIMING };
    c.process(&mut y, &b, &t, f64::INFINITY);

    let win = (0.010 * FS) as usize;
    let mut start = step;
    let mut idx = 0usize;
    while start + win <= x.len() {
        let gain = level_dbfs(&y[start..start + win], 0.0, FS)
            - level_dbfs(&x[start..start + win], 0.0, FS);
        if (gain - target).abs() <= 1.0 {
            return Some(idx as f64 * 10.0);
        }
        start += win;
        idx += 1;
    }
    None
}

#[test]
fn music_release_is_slower() {
    let t50 = recovery_ms(50.0).expect("release 50 ms never recovered to gain_db(-50)");
    let t200 = recovery_ms(200.0).expect("release 200 ms never recovered to gain_db(-50)");
    assert!(
        t200 > 2.5 * t50,
        "recovery times: release 50 ms = {t50} ms, release 200 ms = {t200} ms (need > 2.5x)"
    );
}

#[test]
fn silence_stays_zero() {
    let b = BandFitting { gain_soft_db: 60., gain_loud_db: 25., compression_ratio: 4.5 };
    let mut x = vec![0.0f32; FS as usize];
    let mut c = BandCompressor::new(FS);
    c.process(&mut x, &b, &TEST_TIMING, f64::INFINITY);
    let peak = x.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    assert_eq!(peak, 0.0);
}

#[test]
fn band_ceiling_caps_output() {
    let b = fitting();
    let mut y = sine(1000.0, -15.0, 2.0, FS);
    let mut c = BandCompressor::new(FS);
    c.process(&mut y, &b, &TEST_TIMING, -12.0);
    let out = level_dbfs(&y, 1.5, FS);
    assert!(out <= -11.5 && out >= -13.5, "output {out} dBFS");
}

#[test]
fn band_ceiling_infinite_is_noop() {
    let b = fitting();
    let mut y = sine(1000.0, -15.0, 2.0, FS);
    let mut c = BandCompressor::new(FS);
    c.process(&mut y, &b, &TEST_TIMING, f64::INFINITY);
    let out = level_dbfs(&y, 1.0, FS);
    let expected = -15.0 + gain_db(&b, -15.0);
    assert!((out - expected).abs() <= 1.0, "output {out}, expected {expected}");
}

#[test]
fn blend_weight_selects_fitting() {
    let speech = BandFitting { gain_soft_db: 30., gain_loud_db: 10., compression_ratio: 3. };
    let music = BandFitting { gain_soft_db: 10., gain_loud_db: 10., compression_ratio: 1. };
    let level = -30.0;
    let (gs, gm) = (gain_db(&speech, level), gain_db(&music, level));
    for (w, expected) in [(1.0, gs), (0.0, gm), (0.5, 0.5 * (gs + gm))] {
        let x = sine(1000.0, level, 1.0, FS);
        let mut c = BandCompressor::new(FS);
        let mut g = vec![0f32; x.len()];
        let mut m = vec![0f32; x.len()];
        c.analyze(&x, &speech, &music, w, &TEST_TIMING, &mut g, &mut m);
        let settled = *g.last().unwrap() as f64;
        assert!((settled - expected).abs() <= 0.1, "w={w}: gain {settled:.3} dB, expected {expected:.3} dB");
    }
}

fn cr3() -> BandFitting {
    BandFitting { gain_soft_db: 30., gain_loud_db: 10., compression_ratio: 3. }
}

fn run_gain(x: &[f32], b: &BandFitting, t: &Timing) -> Vec<f32> {
    let mut c = BandCompressor::new(FS);
    let mut g = vec![0f32; x.len()];
    let mut m = vec![0f32; x.len()];
    c.analyze(x, b, b, 1.0, t, &mut g, &mut m);
    g
}

#[test]
fn slow_level_ignores_10hz_modulation() {
    let b = cr3();
    let n = (3.0 * FS) as usize;
    let a0 = 10f64.powf(-30.0 / 20.0);
    let depth = 10f64.powf(6.0 / 20.0) - 1.0; // about +-6 dB
    let x: Vec<f32> = (0..n)
        .map(|i| {
            let t = i as f64 / FS;
            let env = 1.0 + depth * (2.0 * std::f64::consts::PI * 10.0 * t).sin();
            (a0 * env * (2.0 * std::f64::consts::PI * 1000.0 * t).sin()) as f32
        })
        .collect();
    let g = run_gain(&x, &b, &MUSIC_TIMING);
    let tail = &g[n - FS as usize..];
    let mean = tail.iter().map(|&v| v as f64).sum::<f64>() / tail.len() as f64;
    let sd = (tail.iter().map(|&v| (v as f64 - mean).powi(2)).sum::<f64>() / tail.len() as f64).sqrt();
    assert!(sd < 0.5, "gain std {sd:.3} dB");
}

#[test]
fn fast_path_catches_a_step() {
    let b = cr3();
    let step = (2.0 * FS) as usize;
    let mut x = sine(1000.0, -40.0, 3.0, FS);
    let k = 10f64.powf(12.0 / 20.0) as f32;
    for v in &mut x[step..] {
        *v *= k;
    }
    let g = run_gain(&x, &b, &MUSIC_TIMING);
    // 30 ms: the 20/3 ms gain smoothing alone leaves ~0.7 dB at 20 ms even with no detector lag.
    let at = step + (0.030 * FS) as usize;
    let want = gain_db(&b, -28.0);
    assert!((g[at] as f64 - want).abs() <= 1.0, "gain {:.2} dB, want {want:.2} dB", g[at]);
}

#[test]
fn recovers_after_loud_music() {
    let b = cr3();
    let step = (2.0 * FS) as usize;
    let mut x = sine(1000.0, -10.0, 2.0 + 5.0 * MUSIC_TIMING.slow_release_ms / 1000.0 + 0.5, FS);
    let k = 10f64.powf(-30.0 / 20.0) as f32;
    for v in &mut x[step..] {
        *v *= k;
    }
    let g = run_gain(&x, &b, &MUSIC_TIMING);
    let at = step + (5.0 * MUSIC_TIMING.slow_release_ms / 1000.0 * FS) as usize;
    let want = gain_db(&b, -40.0);
    assert!((g[at] as f64 - want).abs() <= 1.0, "gain {:.2} dB, want {want:.2} dB", g[at]);
}

#[test]
fn timing_blend_ends() {
    assert_eq!(blend_timing(0.0), MUSIC_TIMING);
    assert_eq!(blend_timing(1.0), SPEECH_TIMING);
}

/// Gain 250 ms after a 10 ms burst `up` dB above a -50 dBFS tone, relative to the gain just
/// before the burst (dB, negative = still lower).
fn gain_after_burst(t: &Timing, up: f64) -> f64 {
    let b = cr3();
    let start = (2.0 * FS) as usize;
    let mut x = sine(1000.0, -50.0, 3.0, FS);
    let k = 10f64.powf(up / 20.0) as f32;
    for v in &mut x[start..start + (0.010 * FS) as usize] {
        *v *= k;
    }
    let g = run_gain(&x, &b, t);
    g[start + (0.250 * FS) as usize] as f64 - g[start - 1] as f64
}

/// A consonant-like burst (plosive, onset of /s/) jumps far above the slow level in the treble
/// bands. The fast path may follow it down, but it must not hold the gain down for the slow
/// release afterwards: 250 ms later (five fast-release time constants, well inside the 400 ms
/// and 1.5 s slow releases) the gain is within 1 dB of what the slow detector alone (fast path
/// disabled by an endless fast attack) leaves.
#[test]
fn burst_does_not_hold_the_gain_down() {
    for t in [SPEECH_TIMING, MUSIC_TIMING] {
        let slow_only = Timing { fast_attack_ms: 1.0e9, ..t };
        for up in [12.0, 20.0, 30.0] {
            let (with_fast, without) = (gain_after_burst(&t, up), gain_after_burst(&slow_only, up));
            assert!(
                with_fast >= without - 1.0,
                "{t:?} +{up} dB: gain {with_fast:.2} dB at 250 ms, slow detector alone {without:.2} dB"
            );
        }
    }
}
