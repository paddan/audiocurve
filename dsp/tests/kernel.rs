mod support;
use assert_no_alloc::assert_no_alloc;
use audiocurve_dsp::*;
use support::*;

#[global_allocator]
static A: assert_no_alloc::AllocDisabler = assert_no_alloc::AllocDisabler;

const FS: f64 = 48000.0;

fn fit(s: f64, l: f64, cr: f64) -> BandFitting {
    BandFitting { gain_soft_db: s, gain_loud_db: l, compression_ratio: cr }
}

fn run(p: &Params, l: &[f32], r: &[f32], block: usize) -> (Vec<f32>, Vec<f32>) {
    let h = Handle::new(FS, 512).unwrap();
    h.set_params(p);
    run_on(&h, l, r, block)
}

fn run_on(h: &Handle, l: &[f32], r: &[f32], block: usize) -> (Vec<f32>, Vec<f32>) {
    let (mut l, mut r) = (l.to_vec(), r.to_vec());
    for (a, b) in l.chunks_mut(block).zip(r.chunks_mut(block)) {
        unsafe { h.process(a, b) };
    }
    (l, r)
}

fn max_abs(x: &[f32]) -> f32 {
    x.iter().fold(0.0, |m, v| m.max(v.abs()))
}

#[test]
fn unity_is_transparent() {
    let x = sine(1000.0, -20.0, 1.0, FS);
    let (y, _) = run(&Params::unity(), &x, &x, 512);
    let d = level_dbfs(&y, 0.3, FS) + 20.0;
    println!("unity_is_transparent: {d:.3} dB error");
    assert!(d.abs() <= 0.5);
}

#[test]
fn linear_gain_per_band() {
    let mut p = Params::unity();
    p.left = [fit(20.0, 20.0, 1.0); 12];
    p.right = p.left;
    for f in CENTERS {
        let x = sine(f, -40.0, 1.5, FS);
        let (y, _) = run(&p, &x, &x, 512);
        let lv = level_dbfs(&y, 0.5, FS);
        println!("linear_gain_per_band {f}: {lv:.3} dBFS");
        assert!((lv + 20.0).abs() <= 1.0, "{f}: {lv}");
    }
}

#[test]
fn ears_independent() {
    let mut p = Params::unity();
    p.right = [fit(20.0, 20.0, 1.0); 12];
    let x = sine(1000.0, -40.0, 1.0, FS);
    let (l, r) = run(&p, &x, &x, 512);
    let (a, b) = (level_dbfs(&l, 0.3, FS), level_dbfs(&r, 0.3, FS));
    println!("ears_independent: L {a:.3} R {b:.3}");
    assert!((a + 40.0).abs() <= 0.5);
    assert!((b + 20.0).abs() <= 1.0);
}

#[test]
fn mpo_respected() {
    let mut p = Params::unity();
    p.left = [fit(60.0, 25.0, 4.5); 12];
    p.right = p.left;
    p.mpo_dbfs = -3.0;
    let x = noise(10f32.powf(-6.0 / 20.0), (2.0 * FS) as usize, 5);
    let (l, r) = run(&p, &x, &x, 512);
    let m = max_abs(&l).max(max_abs(&r));
    println!("mpo_respected: max {m} (limit {})", 10f32.powf(-3.0 / 20.0));
    assert!(m <= 10f32.powf(-3.0 / 20.0) + 1e-6);
}

#[test]
fn output_gain_applies() {
    let mut p = Params::unity();
    p.output_gain_db = -10.0;
    let x = sine(1000.0, -20.0, 1.0, FS);
    let (y, _) = run(&p, &x, &x, 512);
    let lv = level_dbfs(&y, 0.3, FS);
    println!("output_gain_applies: {lv:.3}");
    assert!((lv + 30.0).abs() <= 0.5);
}

#[test]
fn output_gain_cannot_exceed_ceiling() {
    let mut p = Params::unity();
    p.output_gain_db = 12.0;
    let x = noise(0.9, (2.0 * FS) as usize, 7);
    let (l, r) = run(&p, &x, &x, 512);
    let m = max_abs(&l).max(max_abs(&r));
    println!("output_gain_cannot_exceed_ceiling: max {m}");
    assert!(m <= 10f32.powf(-3.0 / 20.0) + 1e-6);
}

#[test]
fn param_change_has_no_click() {
    let h = Handle::new(FS, 512).unwrap();
    h.set_params(&Params::unity());
    let x = sine(1000.0, -50.0, 0.5, FS);
    let (a, _) = run_on(&h, &x, &x, 512);
    let mut p = Params::unity();
    p.left = [fit(30.0, 30.0, 1.0); 12];
    p.right = p.left;
    h.set_params(&p);
    let n = x.len();
    let x2: Vec<f32> = (n..2 * n)
        .map(|i| (10f64.powf(-50.0 / 20.0) * (2.0 * std::f64::consts::PI * 1000.0 * i as f64 / FS).sin()) as f32)
        .collect();
    let (b, _) = run_on(&h, &x2, &x2, 512);
    let mut y = a;
    y.extend(b);
    let mut worst = 0.0f32;
    for w in y.windows(2) {
        worst = worst.max((w[1] - w[0]).abs());
    }
    let limit = 1.2 * 2.0 * std::f32::consts::PI * 1000.0 / 48000.0 * 10f32.powf(-20.0 / 20.0);
    println!("param_change_has_no_click: max step {worst} limit {limit}");
    assert!(worst <= limit);
}

#[test]
fn nan_input_is_sanitized() {
    let mut p = Params::unity();
    p.left = [fit(20.0, 20.0, 2.0); 12];
    p.right = p.left;
    let x = sine(1000.0, -30.0, 0.5, FS);
    let (reference, _) = run(&p, &x, &x, 512);
    let h = Handle::new(FS, 512).unwrap();
    h.set_params(&p);
    let mut bad = x[..512].to_vec();
    for (i, v) in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY].iter().cycle().take(10).enumerate() {
        bad[100 + i] = *v;
    }
    let mut bad_r = bad.clone();
    unsafe { h.process(&mut bad, &mut bad_r) };
    assert!(bad.iter().chain(bad_r.iter()).all(|v| v.is_finite()));
    let (clean, _) = run_on(&h, &x, &x, 512);
    let a = level_dbfs(&clean, 0.25, FS);
    let b = level_dbfs(&reference, 0.25, FS);
    println!("nan_input_is_sanitized: {a:.3} vs ref {b:.3}");
    assert!((a - b).abs() <= 0.5);
}

#[test]
fn oversized_block_is_chunked() {
    let mut p = Params::unity();
    p.left = [fit(30.0, 15.0, 2.5); 12];
    p.right = p.left;
    let n = 3 * 512 + 7;
    let x = noise(0.1, n, 11);
    let h = Handle::new(FS, 512).unwrap();
    h.set_params(&p);
    let (a, _) = run_on(&h, &x, &x, n);
    let (b, _) = run(&p, &x, &x, 256);
    let d = a.iter().zip(&b).fold(0.0f32, |m, (u, v)| m.max((u - v).abs()));
    println!("oversized_block_is_chunked: max diff {d}");
    assert!(d <= 1e-6);
}

#[test]
fn silence_stays_silent() {
    let mut p = Params::unity();
    p.left = [fit(60.0, 25.0, 4.5); 12];
    p.right = p.left;
    let x = vec![0.0f32; (10.0 * FS) as usize];
    let (l, r) = run(&p, &x, &x, 512);
    let m = max_abs(&l).max(max_abs(&r));
    println!("silence_stays_silent: max {m}");
    assert!(m <= 1e-6);
}

#[test]
fn no_alloc_in_process() {
    let mut p = Params::unity();
    p.left = [fit(40.0, 20.0, 3.0); 12];
    p.right = p.left;
    let h = Handle::new(FS, 512).unwrap();
    h.set_params(&p);
    let mut l = noise(0.1, 512, 1);
    let mut r = noise(0.1, 512, 2);
    l[3] = f32::NAN;
    assert_no_alloc(|| {
        for _ in 0..10 {
            unsafe { h.process(&mut l, &mut r) };
        }
    });
}

#[test]
fn meters_report_in_and_out() {
    let mut p = Params::unity();
    p.output_gain_db = -10.0;
    let x = sine(1000.0, -20.0, 2.0, FS);
    let h = Handle::new(FS, 512).unwrap();
    h.set_params(&p);
    run_on(&h, &x, &x, 512);
    let m = h.meters();
    println!("meters: in {:?} out {:?}", m.in_rms_db, m.out_rms_db);
    assert!((m.in_rms_db[0] + 20.0).abs() <= 0.5);
    assert!((m.out_rms_db[0] + 30.0).abs() <= 0.5);
}

#[test]
fn low_fs_strong_profile_is_finite_and_below_ceiling() {
    let fit_l = |g: f64, cr: f64| fit(g, g * 0.5, cr);
    for fs in [8_000.0, 16_000.0, 22_050.0] {
        let mut p = Params::unity();
        p.left = [
            fit_l(60.0, 4.0), fit_l(60.0, 4.0), fit_l(25.0, 3.0), fit_l(25.0, 3.0), fit_l(25.0, 3.0), fit_l(4.5, 2.0),
            fit_l(4.5, 2.0), fit_l(4.5, 2.0), fit_l(4.5, 2.0), fit_l(4.5, 2.0), fit_l(4.5, 2.0), fit_l(4.5, 2.0),
        ];
        p.right = p.left;
        let h = Handle::new(fs, 512).unwrap();
        h.set_params(&p);
        let mut x = noise(0.5, (fs * 2.0) as usize, 11);
        let s = sine(440.0, -6.0, 2.0, fs);
        for (a, b) in x.iter_mut().zip(&s) {
            *a = (*a + *b).clamp(-1.0, 1.0);
        }
        let (y, _) = run_on(&h, &x, &x, 512);
        let ceiling = 10f32.powf(-3.0 / 20.0) * 1.01;
        assert!(y.iter().all(|v| v.is_finite()), "fs={fs} nonfinite");
        assert!(max_abs(&y) <= ceiling, "fs={fs} max {} > ceiling {ceiling}", max_abs(&y));
        // must not be permanently silent afterwards: a quiet tone still comes out loud
        let tone = sine(440.0, -40.0, 1.0, fs);
        let (z, _) = run_on(&h, &tone, &tone, 512);
        assert!(z.iter().all(|v| v.is_finite()), "fs={fs} nonfinite after");
        assert!(level_dbfs(&z, 0.5, fs) > -45.0, "fs={fs} went silent: {}", level_dbfs(&z, 0.5, fs));
    }
}

#[test]
fn hf_bursts_do_not_pump_bass() {
    let fs = FS;
    let sig = pump_signal(fs, 4.0);
    let mut p = Params::unity();
    p.left = pump_fit(&PUMP_LEFT, 0.5);
    p.right = pump_fit(&PUMP_RIGHT, 0.5);
    p.attack_ms = 5.0;
    p.release_ms = 50.0;
    p.output_gain_db = 0.0;
    p.mpo_dbfs = -3.0;
    let (l, r) = run(&p, &sig, &sig, 512);
    let (ml, mr) = (mod_200_db(&l, fs), mod_200_db(&r, fs));
    println!("pump mod L {ml:.1} dB R {mr:.1} dB");
    assert!(ml <= 3.0 && mr <= 3.0, "bass modulation L {ml:.1} dB, R {mr:.1} dB");
}
