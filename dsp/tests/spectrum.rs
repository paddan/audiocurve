mod support;
use audiocurve_dsp::*;
use audiocurve_dsp::spectrum::SPECTRUM_POINTS;
use support::*;

fn feed(h: &Handle, l: &[f32], r: &[f32]) {
    let (mut l, mut r) = (l.to_vec(), r.to_vec());
    for (a, b) in l.chunks_mut(512).zip(r.chunks_mut(512)) {
        unsafe { h.process(a, b) };
    }
}

fn closest(freqs: &[f32], hz: f32) -> usize {
    (0..freqs.len())
        .min_by(|&a, &b| (freqs[a].ln() - hz.ln()).abs().total_cmp(&(freqs[b].ln() - hz.ln()).abs()))
        .unwrap()
}

#[test]
fn fft_sine_lands_on_right_point() {
    let fs = 48000.0;
    let h = Handle::new(fs, 512).unwrap();
    h.set_params(&Params::unity());
    let x = sine(1000.0, -20.0, 1.0, fs);
    feed(&h, &x, &x);
    let s = h.spectrum();
    let k = closest(&s.freq_hz, 1000.0);
    let v = s.in_db[0][k];
    println!("in@1k {v:.2} dB, out {:.2} dB", s.out_db[0][k]);
    assert!((v + 20.0).abs() <= 1.5, "{v}");
    let max = s.in_db[0].iter().cloned().fold(f32::MIN, f32::max);
    assert_eq!(max, v, "peak must be at the 1 kHz point");
    for i in 0..SPECTRUM_POINTS {
        if s.freq_hz[i] < 500.0 || s.freq_hz[i] > 2000.0 {
            assert!(s.in_db[0][i] <= v - 40.0, "{} Hz: {}", s.freq_hz[i], s.in_db[0][i]);
        }
    }
    assert!((s.out_db[0][k] - v).abs() <= 1.5);
}

#[test]
fn right_channel_is_separate() {
    let fs = 48000.0;
    let h = Handle::new(fs, 512).unwrap();
    h.set_params(&Params::unity());
    let z = vec![0.0f32; fs as usize];
    let x = sine(2000.0, -20.0, 1.0, fs);
    feed(&h, &z, &x);
    let s = h.spectrum();
    for i in 0..SPECTRUM_POINTS {
        assert!(s.in_db[0][i] < -100.0 && s.out_db[0][i] < -100.0, "{i}");
    }
    let k = closest(&s.freq_hz, 2000.0);
    assert!((s.in_db[1][k] + 20.0).abs() <= 1.5);
    assert!((s.out_db[1][k] + 20.0).abs() <= 1.5);
}

#[test]
fn frequencies_log_spaced() {
    for fs in [48000.0, 16000.0] {
        let s = Handle::new(fs, 512).unwrap().spectrum();
        let f = s.freq_hz;
        assert!((f[0] - 20.0).abs() < 0.01);
        assert!(f.windows(2).all(|w| w[1] > w[0]));
        assert!(f[SPECTRUM_POINTS - 1] <= 0.45 * fs as f32 + 1e-3);
        assert!(f[SPECTRUM_POINTS - 1] >= 0.45 * fs as f32 * 0.99 || f[SPECTRUM_POINTS - 1] >= 19900.0);
        let r: Vec<f32> = f.windows(2).map(|w| w[1] / w[0]).collect();
        assert!(r.iter().all(|x| (x - r[0]).abs() < 1e-3));
    }
}

#[test]
fn smoothing_follows_time_constant() {
    let fs = 48000.0;
    let h = Handle::new(fs, 512).unwrap();
    h.set_params(&Params::unity());
    let z = vec![0.0f32; 8192];
    feed(&h, &z, &z);
    let _ = h.spectrum(); // primes at silence
    let x = sine(1000.0, -20.0, 0.1, fs); // 0.1 s -> a = 1-exp(-1/3)
    feed(&h, &x, &x);
    let s = h.spectrum();
    let k = closest(&s.freq_hz, 1000.0);
    // From -140 towards about -20: moved ~28 % of the way, so still far below -20.
    assert!(s.in_db[0][k] < -80.0 && s.in_db[0][k] > -140.0, "{}", s.in_db[0][k]);
}

#[test]
fn null_safe_ffi() {
    unsafe {
        ac_kernel_read_spectrum(std::ptr::null(), std::ptr::null_mut());
        let mut s = Spectrum::default();
        ac_kernel_read_spectrum(std::ptr::null(), &mut s);
    }
}
