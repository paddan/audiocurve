mod support;
use audiocurve_dsp::biquad::Biquad;
use audiocurve_dsp::filterbank::{BandBuffers, Filterbank};
use audiocurve_dsp::BAND_COUNT;
use support::*;

fn sum_bands(out: &BandBuffers, n: usize) -> Vec<f32> {
    (0..n).map(|i| out.bands.iter().map(|b| b[i]).sum()).collect()
}

#[test]
fn bands_sum_flat() {
    for fs in [44_100.0, 48_000.0, 96_000.0] {
        let mut worst = 0.0f64;
        for f in [50., 100., 177., 300., 612., 1000., 1732., 3000., 4899., 8000., 9798., 15000.] {
            let x = sine(f, -20.0, 1.0, fs);
            let mut fb = Filterbank::new(fs);
            let mut out = BandBuffers::new(x.len());
            fb.split(&x, &mut out);
            let sum = sum_bands(&out, x.len());
            let dev = level_dbfs(&sum, 0.25, fs) + 20.0;
            worst = worst.max(dev.abs());
            assert!(dev.abs() <= 0.5, "fs={fs} f={f} dev={dev}");
        }
        println!("fs={fs} worst flat-sum deviation = {worst:.5} dB");
    }
}

#[test]
fn band_isolation() {
    let fs = 48_000.0;
    let x = sine(1000.0, -20.0, 1.0, fs);
    let mut fb = Filterbank::new(fs);
    let mut out = BandBuffers::new(x.len());
    fb.split(&x, &mut out);
    let l = |i: usize| level_dbfs(&out.bands[i][..x.len()], 0.25, fs);
    for i in [0, 3, 4, 5, 11] {
        println!("band[{i}] = {:.2} dBFS", l(i));
    }
    // 1 kHz lies close to both edges of band 4 (866..1225 Hz); LR4 costs ~8 dB there.
    assert!(l(4) >= -30.0, "band4 {}", l(4));
    for i in 0..BAND_COUNT {
        if i != 4 {
            assert!(l(4) > l(i), "band4 {} not above band{i} {}", l(4), l(i));
        }
    }
    assert!(l(0) < -60.0, "band0 {}", l(0));
    assert!(l(11) < -60.0, "band11 {}", l(11));
}

#[test]
fn reset_clears_state() {
    let fs = 48_000.0;
    let mut fb = Filterbank::new(fs);
    let n = 4800;
    let mut out = BandBuffers::new(n);
    fb.split(&noise(0.5, n, 7), &mut out);
    fb.reset();
    fb.split(&vec![0.0; n], &mut out);
    for b in 0..BAND_COUNT {
        assert!(out.bands[b][..n].iter().all(|&v| v == 0.0), "band {b}");
    }
}

#[test]
fn biquad_lowpass_basics() {
    let fs = 48_000.0;
    let mut lp = Biquad::lowpass(1000.0, fs, std::f64::consts::FRAC_1_SQRT_2);
    let x = sine(50.0, -10.0, 0.5, fs);
    let y: Vec<f32> = x.iter().map(|&v| lp.process(v)).collect();
    assert!((level_dbfs(&y, 0.1, fs) + 10.0).abs() < 0.1);
}

const ALL_FS: [f64; 11] = [
    8_000.0, 11_025.0, 16_000.0, 22_050.0, 32_000.0, 44_100.0, 48_000.0, 88_200.0, 96_000.0, 192_000.0, 384_000.0,
];

#[test]
fn bands_sum_flat_every_accepted_fs() {
    for fs in ALL_FS {
        for f in [50., 100., 177., 300., 612., 1000., 1732., 3000., 4899., 8000., 9798., 15000.] {
            if f >= 0.4 * fs {
                continue;
            }
            let x = sine(f, -20.0, 1.0, fs);
            let mut fb = Filterbank::new(fs);
            let mut out = BandBuffers::new(x.len());
            fb.split(&x, &mut out);
            assert!(out.bands.iter().all(|b| b[..x.len()].iter().all(|v| v.is_finite())), "fs={fs} f={f} nonfinite");
            let sum = sum_bands(&out, x.len());
            let dev = level_dbfs(&sum, 0.25, fs) + 20.0;
            assert!(dev.abs() <= 0.5, "fs={fs} f={f} dev={dev}");
        }
    }
}

#[test]
fn stable_and_silent_every_accepted_fs() {
    for fs in ALL_FS {
        let n = (fs * 2.0) as usize;
        let mut fb = Filterbank::new(fs);
        let mut out = BandBuffers::new(n);
        // loud noise burst must stay finite and bounded
        fb.split(&noise(1.0, n, 3), &mut out);
        for b in &out.bands {
            assert!(b[..n].iter().all(|v| v.is_finite() && v.abs() < 20.0), "fs={fs} unstable");
        }
        // silence stays silent
        fb.reset();
        fb.split(&vec![0.0; n], &mut out);
        for b in &out.bands {
            assert!(b[..n].iter().all(|&v| v == 0.0), "fs={fs} silence not silent");
        }
    }
}
