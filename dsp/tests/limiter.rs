mod support;
use audiocurve_dsp::limiter::Limiter;
use support::*;

const FS: f64 = 48000.0;

fn run(l: &mut [f32], r: &mut [f32], ceiling: f64, block: usize) -> Limiter {
    let mut lim = Limiter::new(FS, block);
    for (cl, cr) in l.chunks_mut(block).zip(r.chunks_mut(block)) {
        lim.process(cl, cr, ceiling);
    }
    lim
}

fn loud_input() -> (Vec<f32>, Vec<f32>) {
    let n = (2.0 * FS) as usize;
    let mut l = noise(4.0, n, 1);
    let mut r = noise(4.0, n, 2);
    for i in (1000..n).step_by(7919) {
        l[i] = 10.0;
        r[i + 1] = -10.0;
    }
    (l, r)
}

fn max_abs(x: &[f32]) -> f32 {
    x.iter().fold(0.0, |m, v| m.max(v.abs()))
}

#[test]
fn limiter_never_exceeds_ceiling() {
    let (mut l, mut r) = loud_input();
    run(&mut l, &mut r, -3.0, 512);
    let c = 10f64.powf(-3.0 / 20.0);
    let m = max_abs(&l).max(max_abs(&r)) as f64;
    println!("max out {m} ceiling {c}");
    assert!(m <= c + 1e-6);
}

#[test]
fn limiter_absolute_ceiling() {
    let (mut l, mut r) = loud_input();
    run(&mut l, &mut r, 0.0, 333);
    let c = 10f64.powf(-1.0 / 20.0);
    let m = max_abs(&l).max(max_abs(&r)) as f64;
    println!("max out {m} ceiling {c}");
    assert!(m <= c + 1e-6);
}

#[test]
fn limiter_transparent_below() {
    let mut l = sine(1000.0, -20.0, 1.0, FS);
    let mut r = l.clone();
    let lim = run(&mut l, &mut r, -3.0, 256);
    assert_eq!(lim.latency_frames(), (0.002 * FS).round() as usize);
    let d = level_dbfs(&l, 0.1, FS);
    assert!((d + 20.0).abs() < 0.1, "level {d}");
}

#[test]
fn limiter_nan_inf_safe() {
    let n = 20_000;
    let mut l = noise(0.5, n, 3);
    let mut r = noise(0.5, n, 4);
    for i in (100..n).step_by(97) {
        l[i] = f32::NAN;
        r[i + 1] = f32::INFINITY;
        l[i + 2] = f32::NEG_INFINITY;
        r[i + 3] = -f32::NAN;
    }
    run(&mut l, &mut r, -3.0, 512);
    let c = 10f32.powf(-3.0 / 20.0);
    assert!(l.iter().chain(r.iter()).all(|y| y.is_finite() && y.abs() <= c + 1e-6));
}

#[test]
fn limiter_odd_block_sizes_and_empty() {
    let mut lim = Limiter::new(FS, 64);
    lim.process(&mut [], &mut [], -3.0);
    let mut a = [5.0f32; 1];
    let mut b = [5.0f32; 1];
    lim.process(&mut a, &mut b, -3.0);
    assert_eq!((a[0], b[0]), (0.0, 0.0)); // still inside the delay line
    let c = 10f32.powf(-3.0 / 20.0);
    for (k, &len) in [1usize, 7, 64, 3, 33, 64, 2].iter().cycle().take(200).enumerate() {
        let mut x = noise(4.0, len, k as u64 + 1);
        let mut y = x.clone();
        lim.process(&mut x, &mut y, -3.0);
        assert!(x.iter().chain(y.iter()).all(|v| v.abs() <= c + 1e-6));
    }
    lim.reset();
}

#[test]
fn limiter_channels_independent() {
    let n = (1.0 * FS) as usize;
    let mut l = noise(0.9, n, 3);
    let mut r = sine(1000.0, -20.0, 1.0, FS);
    run(&mut l, &mut r, -3.0, 512);
    let lvl = level_dbfs(&r, 0.1, FS);
    assert!((lvl - -20.0).abs() <= 0.1, "right level {lvl}");
}

#[test]
fn limiter_soft_knee_eases_just_below_ceiling() {
    // Knee 3 dB around -3: a -3.5 dBFS sine is eased to about -3.67, a -6 dBFS one is untouched.
    let mut l = sine(1000.0, -3.5, 1.0, FS);
    let mut r = sine(1000.0, -6.0, 1.0, FS);
    run(&mut l, &mut r, -3.0, 512);
    let (a, b) = (level_dbfs(&l, 0.3, FS), level_dbfs(&r, 0.3, FS));
    assert!((a - -3.667).abs() < 0.05, "knee level {a}");
    assert!((b - -6.0).abs() < 0.05, "below knee {b}");
}
