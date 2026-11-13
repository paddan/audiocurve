mod support;
#[path = "support/measure/mod.rs"]
mod measure;
use audiocurve_dsp::limiter::Limiter;
use measure::{metrics::true_peak, signals};
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
    assert_eq!(lim.latency_frames(), (0.005 * FS).round() as usize + 16);
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
    // Knee 3 dB around the guarded ceiling -3.2: a -3.5 dBFS sine is eased to about -3.74,
    // a -6 dBFS one is untouched.
    let mut l = sine(1000.0, -3.5, 1.0, FS);
    let mut r = sine(1000.0, -6.0, 1.0, FS);
    run(&mut l, &mut r, -3.0, 512);
    let (a, b) = (level_dbfs(&l, 0.3, FS), level_dbfs(&r, 0.3, FS));
    assert!((a - -3.74).abs() < 0.05, "knee level {a}");
    assert!((b - -6.0).abs() < 0.05, "below knee {b}");
}

// ---- true-peak limiter (2.1) ----

const FS_TP: f64 = 44_100.0;
const TP_CEILING_DBFS: f64 = -3.0;

/// The two worst-case signals at 44.1 kHz: a full-scale 11 025 Hz (fs/4) sine at 45 degrees
/// (samples at -3 dBFS, true peak 0 dBFS) and pink noise with 30 ms bursts to +6 dBFS.
fn tp_signals() -> Vec<Vec<f32>> {
    let n = (3.0 * FS_TP) as usize;
    let sine = signals::sine(FS_TP / 4.0, 0.0, std::f64::consts::FRAC_PI_4, 3.0, FS_TP);
    let mut pink = signals::pink(-14.0, n, 7);
    let (period, len) = ((0.25 * FS_TP) as usize, (0.03 * FS_TP) as usize);
    for start in (period / 2..n - len).step_by(period) {
        signals::normalize_peak(&mut pink[start..start + len], 6.0);
    }
    vec![sine, signals::to_f32(&pink)]
}

fn limit(x: &[f32], fs: f64, block: usize) -> Vec<f32> {
    let (mut l, mut r) = (x.to_vec(), x.to_vec());
    let mut lim = Limiter::new(fs, block);
    for (cl, cr) in l.chunks_mut(block).zip(r.chunks_mut(block)) {
        lim.process(cl, cr, TP_CEILING_DBFS);
    }
    assert_eq!(l, r);
    l
}

#[test]
fn true_peak_stays_below_ceiling() {
    for (k, x) in tp_signals().iter().enumerate() {
        let y = limit(x, FS_TP, 512);
        let tp = 20.0 * true_peak(&y).log10();
        println!("signal {k}: input true peak {:.2} dBFS, output {tp:.3} dBFS", 20.0 * true_peak(x).log10());
        assert!(tp <= TP_CEILING_DBFS, "signal {k}: true peak {tp} dBFS > {TP_CEILING_DBFS}");
    }
}

#[test]
fn hard_clamp_never_engages() {
    // The clamp sets |y| to exactly c, so an output that stays strictly below c (here by
    // 0.1 dB) proves the pre-clamp value never exceeded c.
    let limit_lin = 10f32.powf((TP_CEILING_DBFS - 0.1) as f32 / 20.0);
    for (k, x) in tp_signals().iter().enumerate() {
        let y = limit(x, FS_TP, 333);
        let m = max_abs(&y);
        println!("signal {k}: max |y| {:.3} dBFS", 20.0 * m.log10());
        assert!(m <= limit_lin, "signal {k}: |y| {m} reached the clamp region");
    }
}

#[test]
fn attack_is_smooth() {
    // 1 kHz sine stepping from -20 to +6 dBFS; one-frame blocks expose the per-sample gain.
    let mut x = sine(1000.0, -20.0, 0.5, FS);
    x.extend(sine(1000.0, 6.0, 1.0, FS)); // 0.5 s is a whole number of periods
    let mut lim = Limiter::new(FS, 1);
    let l_frames = (0.005 * FS).round() as f32;
    let mut g = Vec::with_capacity(x.len());
    for &v in &x {
        let (mut a, mut b) = ([v], [v]);
        lim.process(&mut a, &mut b, TP_CEILING_DBFS);
        g.push(10f32.powf(-lim.last_reduction_db()[0] / 20.0));
    }
    let total = 1.0 - g.iter().fold(1.0f32, |m, &v| m.min(v));
    assert!(total > 0.5, "total reduction {total}");
    let worst = g.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0f32, f32::max);
    println!("total reduction {total}, worst per-sample step {worst}, bound {}", total / l_frames);
    assert!(worst <= total / l_frames + 1e-6, "step {worst} > {}", total / l_frames);
}

#[test]
fn latency_matches_delay() {
    for fs in [44_100.0, 48_000.0, 96_000.0] {
        let mut lim = Limiter::new(fs, 64);
        let lat = lim.latency_frames();
        assert_eq!(lat, (0.005 * fs).round() as usize + 16, "fs {fs}");
        let mut l = vec![0.0f32; 4096];
        l[100] = 0.25;
        let mut r = l.clone();
        for (cl, cr) in l.chunks_mut(64).zip(r.chunks_mut(64)) {
            lim.process(cl, cr, TP_CEILING_DBFS);
        }
        let peak = l.iter().enumerate().fold((0, 0.0f32), |a, (i, &v)| if v.abs() > a.1 { (i, v.abs()) } else { a }).0;
        println!("fs {fs}: latency {lat}, impulse at +{}", peak - 100);
        assert_eq!(peak - 100, lat, "fs {fs}");
        assert!((l[peak] - 0.25).abs() < 1e-6, "impulse changed: {}", l[peak]);
    }
}

