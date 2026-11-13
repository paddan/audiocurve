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

/// Selects the kernel time constants through `speech_weight`: attack 5 ms = Speech (5/50 ms),
/// anything else = Music (10/200 ms). Both fitting sets get the speech fitting.
fn set_timing(p: &mut Params, attack_ms: f64) {
    p.music_left = p.left;
    p.music_right = p.right;
    p.speech_weight = if attack_ms == 5.0 { 1.0 } else { 0.0 };
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
fn capture_does_not_allocate() {
    // Capture is always on; process must not allocate even while the spectrum is being read.
    let h = Handle::new(FS, 512).unwrap();
    h.set_params(&Params::unity());
    let mut l = noise(0.1, 512, 1);
    let mut r = noise(0.1, 512, 2);
    assert_no_alloc(|| {
        for _ in 0..20 {
            unsafe { h.process(&mut l, &mut r) };
        }
    });
    let s = h.spectrum();
    assert!(s.in_db[0].iter().any(|&v| v > -100.0), "capture should have data");
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
    let sig = pump_signal(fs, 10.0);
    // (strength, attack ms, release ms, max allowed bass modulation dB)
    for (s, atk, rel, max) in [(0.5, 5.0, 50.0, 3.0), (0.5, 10.0, 200.0, 3.0), (1.0, 5.0, 50.0, 4.5), (1.0, 10.0, 200.0, 4.5)] {
        let mut p = Params::unity();
        p.left = pump_fit(&PUMP_LEFT, s);
        p.right = pump_fit(&PUMP_RIGHT, s);
        set_timing(&mut p, atk);
        p.output_gain_db = 0.0;
        p.mpo_dbfs = -3.0;
        let (l, r) = run(&p, &sig, &sig, 512);
        let (ml, mr) = (mod_200_db(&l, fs), mod_200_db(&r, fs));
        println!("pump s={s} rel={rel}: L {ml:.1} dB R {mr:.1} dB");
        assert!(ml <= max && mr <= max, "s={s} rel={rel}: bass modulation L {ml:.1} dB, R {mr:.1} dB > {max}");
    }
}

#[test]
fn loud_tones_stay_clean_under_output_budget() {
    // Loud enough that the cross-band output budget engages; the cap must not ripple
    // with the waveform (that is audible as distortion).
    for f in [50.0, 100.0, 200.0, 1000.0] {
        let mut p = Params::unity();
        p.left = pump_fit(&PUMP_LEFT, 0.5);
        p.right = pump_fit(&PUMP_RIGHT, 0.5);
        let x = sine(f, -6.0, 6.0, FS);
        let (l, r) = run(&p, &x, &x, 512);
        // Last second only, after the headroom AGC has settled.
        let start = l.len() - FS as usize;
        let len = (((l.len() - start) as f64 * f / FS).floor() * FS / f).round() as usize;
        let (tl, tr) = (thd_n_db(&l[start..start + len], f, FS), thd_n_db(&r[start..start + len], f, FS));
        println!("thd {f} Hz: L {tl:.1} dB R {tr:.1} dB");
        assert!(tl <= -70.0 && tr <= -70.0, "{f} Hz: THD+N L {tl:.1} dB, R {tr:.1} dB");
    }
}

/// Energy (sum of Goertzel bin powers) of `x` from `lo` to `hi` Hz in 4 Hz steps.
fn band_energy(x: &[f32], lo: f64, hi: f64) -> f64 {
    let mut e = 0.0;
    let mut f = lo;
    while f <= hi {
        let c = 2.0 * (2.0 * std::f64::consts::PI * f / FS).cos();
        let (mut s1, mut s2) = (0.0f64, 0.0f64);
        for v in x {
            let s0 = *v as f64 + c * s1 - s2;
            s2 = s1;
            s1 = s0;
        }
        e += s1 * s1 + s2 * s2 - c * s1 * s2;
        f += 4.0;
    }
    e
}

#[test]
fn dense_treble_bursts_stay_clean() {
    // 80-tone 5-8 kHz bursts (sibilant / distorted-guitar-like treble). The kernel must not
    // create much energy below the input band (products of fast gain changes or clipping).
    let n = (FS * 3.0) as usize;
    let tones: Vec<(f64, f64)> = (0..80).map(|i| (5003.0 + 37.0 * i as f64, i as f64 * 2.399)).collect();
    for peak_db in [-12.0, -6.0] {
        let norm = 10f64.powf(peak_db / 20.0) / (tones.len() as f64).sqrt() / 3.0;
        let sig: Vec<f32> = (0..n)
            .map(|k| {
                let t = k as f64 / FS;
                let ph = t % 0.25;
                let env = if ph < 0.005 { ph / 0.005 } else if ph < 0.075 { 1.0 } else if ph < 0.080 { (0.080 - ph) / 0.005 } else { 0.0 };
                let s: f64 = tones.iter().map(|(f, p)| (2.0 * std::f64::consts::PI * f * t + p).sin()).sum();
                (norm * env * s) as f32
            })
            .collect();
        for (atk, rel) in [(5.0, 50.0), (10.0, 200.0)] {
            let mut p = Params::unity();
            p.left = pump_fit(&PUMP_LEFT, 0.5);
            p.right = pump_fit(&PUMP_RIGHT, 0.5);
            set_timing(&mut p, atk);
            let (l, _) = run(&p, &sig, &sig, 512);
            let w = &l[n / 3..];
            let ratio = 10.0 * (band_energy(w, 200.0, 4000.0) / band_energy(w, 5000.0, 8000.0)).log10();
            println!("treble bursts {peak_db} dBFS rel {rel}: products {ratio:.1} dB");
            assert!(ratio <= -58.0, "{peak_db} dBFS, release {rel}: products {ratio:.1} dB");
        }
    }
}

#[test]
fn reported_latency_matches_signal_delay() {
    // A unity kernel should pass a click through delayed by exactly the reported latency
    // (the filterbank adds phase shift but no bulk delay), so hosts can compensate.
    let h = Handle::new(FS, 512).unwrap();
    h.set_params(&Params::unity());
    let mut x = vec![0f32; 4096];
    x[100] = 0.5;
    let (y, _) = run_on(&h, &x, &x, 512);
    let peak = y.iter().enumerate().max_by(|a, b| a.1.abs().total_cmp(&b.1.abs())).unwrap().0;
    let lat = h.latency_frames() as usize;
    println!("latency {lat}, click peak at {}", peak - 100);
    assert!((peak as i64 - 100 - lat as i64).abs() <= 8, "peak at +{}, latency {lat}", peak - 100);
}

#[test]
fn limiter_off_passes_above_mpo_but_clamps_full_scale() {
    let mut p = Params::unity();
    p.left = [fit(20.0, 20.0, 1.0); 12];
    p.right = p.left;
    p.mpo_dbfs = -10.0;
    p.limiter_enabled = 0.0;
    // -12 dBFS + 20 dB = +8 dBFS wanted: above the MPO, clamped at full scale.
    let x = sine(1000.0, -12.0, 1.0, FS);
    let (l, _) = run(&p, &x, &x, 512);
    let m = max_abs(&l);
    println!("limiter off: max {m}");
    assert!(m > 10f32.powf(-10.0 / 20.0) * 1.5 && m <= 1.0, "max {m}");
    // A quieter tone passes linearly, well above the -10 dBFS MPO.
    let x = sine(1000.0, -26.0, 1.0, FS);
    let (l, _) = run(&p, &x, &x, 512);
    let lv = level_dbfs(&l, 0.3, FS);
    assert!((lv + 6.0).abs() <= 1.0, "level {lv}");
}

#[test]
fn limiter_toggle_keeps_latency() {
    let h = Handle::new(FS, 512).unwrap();
    let lat = h.latency_frames();
    let mut p = Params::unity();
    p.limiter_enabled = 0.0;
    h.set_params(&p);
    let mut x = vec![0f32; 4096];
    x[100] = 0.5;
    let (y, _) = run_on(&h, &x, &x, 512);
    let peak = y.iter().enumerate().max_by(|a, b| a.1.abs().total_cmp(&b.1.abs())).unwrap().0;
    assert!((peak as i64 - 100 - lat as i64).abs() <= 8, "peak at +{}, latency {lat}", peak - 100);
}

#[test]
fn reduction_meter_reports_safety_stage() {
    let mut p = Params::unity();
    p.left = [fit(20.0, 20.0, 1.0); 12];
    p.right = [fit(0.0, 0.0, 1.0); 12];
    p.mpo_dbfs = -10.0;
    // Left: -12 dBFS + 20 dB wants +8 dBFS, ~18 dB above the -10 MPO. Right: -30 dBFS, untouched.
    let h = Handle::new(FS, 512).unwrap();
    h.set_params(&p);
    run_on(&h, &sine(1000.0, -12.0, 2.0, FS), &sine(1000.0, -30.0, 2.0, FS), 512);
    let m = h.meters();
    println!("reduction {:?}", m.reduction_db);
    assert!(m.reduction_db[0] > 15.0 && m.reduction_db[0] < 21.0, "left {}", m.reduction_db[0]);
    assert!(m.reduction_db[1] < 0.5, "right {}", m.reduction_db[1]);
    // Limiter off: nothing is reduced by the safety stage (only the 0 dBFS clamp remains).
    p.limiter_enabled = 0.0;
    h.set_params(&p);
    run_on(&h, &sine(1000.0, -12.0, 2.0, FS), &sine(1000.0, -30.0, 2.0, FS), 512);
    let m = h.meters();
    assert!(m.reduction_db[0] < 0.1 && m.reduction_db[1] < 0.1, "off {:?}", m.reduction_db);
}

#[test]
fn input_gain_applies_before_bands_and_in_meter() {
    let mut p = Params::unity();
    p.input_gain_db = -10.0;
    let x = sine(1000.0, -20.0, 2.0, FS);
    let h = Handle::new(FS, 512).unwrap();
    h.set_params(&p);
    let (l, _) = run_on(&h, &x, &x, 512);
    let m = h.meters();
    // The In meter follows the input level: it shows what the bands get.
    assert!((m.in_rms_db[0] + 30.0).abs() <= 0.5, "in {}", m.in_rms_db[0]);
    let lv = level_dbfs(&l, 0.5, FS);
    assert!((lv + 30.0).abs() <= 0.3, "out {lv}");
}

#[test]
fn input_gain_drives_compression_less() {
    // A compressing band (2:1 above -60): 10 dB less input gives ~5 dB less output.
    let mut p = Params::unity();
    p.left = [fit(20.0, 0.0, 2.0); 12];
    p.right = p.left;
    p.mpo_dbfs = -1.0;
    let x = sine(1000.0, -20.0, 2.0, FS);
    let (a, _) = run(&p, &x, &x, 512);
    p.input_gain_db = -10.0;
    let (b, _) = run(&p, &x, &x, 512);
    let d = level_dbfs(&a, 1.0, FS) - level_dbfs(&b, 1.0, FS);
    assert!(d > 3.5 && d < 7.0, "output drop {d} dB");
}

#[test]
fn auto_mode_reports_speech_value_and_stays_bounded() {
    let mut p = Params::unity();
    p.left = [fit(20.0, 5.0, 2.0); 12];
    p.right = p.left;
    p.auto_mode = 1.0;
    let h = Handle::new(FS, 512).unwrap();
    h.set_params(&p);
    // A steady tone is music-like (no dips): the speech value falls toward 0.
    let (l, _) = run_on(&h, &sine(1000.0, -20.0, 12.0, FS), &sine(1000.0, -20.0, 12.0, FS), 512);
    let m = h.meters();
    assert!(m.speech < 0.1, "speech {}", m.speech);
    assert!(l.iter().all(|v| v.is_finite()));
}

#[test]
fn inactive_passes_input_through_delayed() {
    let mut p = Params::unity();
    p.left = [fit(20.0, 20.0, 1.0); 12];
    p.right = p.left;
    p.active = 0.0;
    let h = Handle::new(FS, 512).unwrap();
    h.set_params(&p);
    let x = sine(1000.0, -20.0, 0.5, FS);
    let (l, r) = run_on(&h, &x, &x, 512);
    let lat = h.latency_frames() as usize;
    for i in lat..x.len() {
        assert!((l[i] - x[i - lat]).abs() < 1e-6 && (r[i] - x[i - lat]).abs() < 1e-6, "sample {i}");
    }
}

#[test]
fn activity_switch_is_a_smooth_crossfade() {
    // +20 dB when active, 0 dB when inactive: the switch must glide, not jump.
    let mut p = Params::unity();
    p.left = [fit(20.0, 20.0, 1.0); 12];
    p.right = p.left;
    p.mpo_dbfs = -1.0;
    let h = Handle::new(FS, 512).unwrap();
    h.set_params(&p);
    let x = sine(200.0, -30.0, 1.0, FS);
    let (a, _) = run_on(&h, &x, &x, 512);
    p.active = 0.0;
    h.set_params(&p);
    let (b, _) = run_on(&h, &x, &x, 512);
    let lat = h.latency_frames();
    assert_eq!(lat, Handle::new(FS, 512).unwrap().latency_frames());
    // Steady states: about -10 dBFS active, -30 dBFS inactive.
    assert!((level_dbfs(&a, 0.5, FS) + 10.0).abs() < 1.0, "active {}", level_dbfs(&a, 0.5, FS));
    assert!((level_dbfs(&b, 0.5, FS) + 30.0).abs() < 1.0, "inactive {}", level_dbfs(&b, 0.5, FS));
    // No click: the largest sample step across the switch stays near a 200 Hz sine's own.
    let joined: Vec<f32> = a.iter().chain(b.iter()).copied().collect();
    let max_step = joined.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0f32, f32::max);
    let sine_step = 2.0 * std::f32::consts::PI * 200.0 / FS as f32 * 10f32.powf(-10.0 / 20.0);
    assert!(max_step < 1.2 * sine_step, "step {max_step} vs {sine_step}");
}

#[test]
fn identical_fittings_match_single_fitting() {
    // With both sets equal the weight only moves the time constants, never the static gain:
    // the settled output must not depend on it. Weight 1 must also ignore the music set.
    let x = sine(1000.0, -30.0, 2.0, FS);
    let mut base = Params::unity();
    base.left = [fit(30.0, 15.0, 2.5); 12];
    base.right = base.left;
    base.music_left = base.left;
    base.music_right = base.right;
    let tail = |y: &[f32]| y[y.len() - 9600..].to_vec();
    let mut outs = Vec::new();
    for w in [0.0, 0.5, 1.0] {
        let mut p = base;
        p.speech_weight = w;
        outs.push(tail(&run(&p, &x, &x, 512).0));
    }
    for o in &outs[1..] {
        let d = outs[0].iter().zip(o).fold(0f32, |m, (u, v)| m.max((u - v).abs()));
        println!("settled diff across weights: {d:e}");
        assert!(d < 1e-3, "settled output depends on the weight: {d}");
    }
    let mut p = base;
    p.speech_weight = 1.0;
    p.music_left = [fit(0.0, 0.0, 1.0); 12];
    p.music_right = p.music_left;
    let d = outs[2].iter().zip(tail(&run(&p, &x, &x, 512).0)).fold(0f32, |m, (u, v)| m.max((u - v).abs()));
    assert!(d < 1e-6, "weight 1 must ignore the music set: {d}");
}

#[test]
fn mode_switch_is_click_free() {
    let mut p = Params::unity();
    p.left = [fit(40.0, 20.0, 3.0); 12];
    p.right = p.left;
    p.music_left = [fit(10.0, 10.0, 1.0); 12];
    p.music_right = p.music_left;
    p.speech_weight = 0.0;
    let x = sine(1000.0, -20.0, 3.0, FS);
    let h = Handle::new(FS, 512).unwrap();
    h.set_params(&p);
    let half = x.len() / 2 / 512 * 512;
    let (mut l, mut r) = (x.clone(), x.clone());
    for (i, (a, b)) in l.chunks_mut(512).zip(r.chunks_mut(512)).enumerate() {
        if i * 512 == half {
            p.speech_weight = 1.0;
            h.set_params(&p);
        }
        unsafe { h.process(a, b) };
    }
    let steps = |s: &[f32]| s.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0f32, f32::max);
    // Steady reference: the largest step of the signal after settling, before the switch.
    let steady = steps(&l[half / 2..half]).max(steps(&l[l.len() - 20_000..]));
    let around = steps(&l[half - 2048..half + 20_000]);
    assert!(around <= 2.0 * steady, "step {around} > 2 x steady {steady}");
}
