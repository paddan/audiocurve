//! Deterministic test signals (f32 buffers at the caller's sample rate).

use super::support;
use std::f64::consts::PI;

pub fn db_to_lin(db: f64) -> f64 {
    10f64.powf(db / 20.0)
}

pub fn sine(freq: f64, level_dbfs: f64, phase: f64, seconds: f64, fs: f64) -> Vec<f32> {
    let n = (seconds * fs).round() as usize;
    let amp = db_to_lin(level_dbfs);
    (0..n).map(|i| (amp * (2.0 * PI * freq * i as f64 / fs + phase).sin()) as f32).collect()
}

/// Scales so the maximum absolute value equals `peak_dbfs`.
pub fn normalize_peak(x: &mut [f64], peak_dbfs: f64) {
    let m = x.iter().fold(0f64, |a, v| a.max(v.abs()));
    let g = db_to_lin(peak_dbfs) / m;
    x.iter_mut().for_each(|v| *v *= g);
}

fn scale_rms(x: &mut [f64], rms_dbfs: f64) {
    let ms = x.iter().map(|v| v * v).sum::<f64>() / x.len() as f64;
    let g = db_to_lin(rms_dbfs) / ms.sqrt();
    x.iter_mut().for_each(|v| *v *= g);
}

/// Pink noise (Paul Kellet's filter on uniform white noise) at the given RMS level.
pub fn pink(rms_dbfs: f64, n: usize, seed: u64) -> Vec<f64> {
    let w = support::noise(1.0, n, seed);
    let (mut b0, mut b1, mut b2, mut b3, mut b4, mut b5, mut b6) = (0f64, 0f64, 0f64, 0f64, 0f64, 0f64, 0f64);
    let mut y: Vec<f64> = w
        .iter()
        .map(|&v| {
            let v = v as f64;
            b0 = 0.99886 * b0 + v * 0.0555179;
            b1 = 0.99332 * b1 + v * 0.0750759;
            b2 = 0.96900 * b2 + v * 0.1538520;
            b3 = 0.86650 * b3 + v * 0.3104856;
            b4 = 0.55000 * b4 + v * 0.5329522;
            b5 = -0.7616 * b5 - v * 0.0168980;
            let o = b0 + b1 + b2 + b3 + b4 + b5 + b6 + v * 0.5362;
            b6 = v * 0.115926;
            o
        })
        .collect();
    scale_rms(&mut y, rms_dbfs);
    y
}

pub fn to_f32(x: &[f64]) -> Vec<f32> {
    x.iter().map(|&v| v as f32).collect()
}

/// Pink noise amplitude-modulated by 2, 5 and 8 Hz sines; the modulation depth in dB
/// follows their mean and peaks at +-10 dB.
pub fn modulated_pink(rms_dbfs: f64, seconds: f64, fs: f64) -> Vec<f32> {
    let n = (seconds * fs) as usize;
    let p = pink(rms_dbfs, n, 11);
    p.iter()
        .enumerate()
        .map(|(i, &v)| {
            let t = i as f64 / fs;
            let m = ((2.0 * PI * 2.0 * t).sin() + (2.0 * PI * 5.0 * t).sin() + (2.0 * PI * 8.0 * t).sin()) / 3.0;
            (v * db_to_lin(10.0 * m)) as f32
        })
        .collect()
}

/// Dense program: pink noise at `DENSE_BED_DBFS` RMS plus a 30 ms decaying noise burst every 250 ms
/// whose peak is -0.5 dBFS.
/// Pink bed level of the dense signal; -20 kept because no tried level (-20..-60) got active_pct <= 70 (see fix report).
pub const DENSE_BED_DBFS: f64 = -20.0;

pub fn dense(seconds: f64, fs: f64) -> Vec<f32> {
    let n = (seconds * fs) as usize;
    let mut bursts = vec![0f64; n];
    let white = support::noise(1.0, n, 23);
    let (period, len) = ((0.25 * fs) as usize, (0.03 * fs) as usize);
    for i in 0..n {
        let k = i % period;
        if k < len {
            bursts[i] = white[i] as f64 * (-(k as f64) / (0.008 * fs)).exp();
        }
    }
    normalize_peak(&mut bursts, -0.5);
    let p = pink(DENSE_BED_DBFS, n, 5);
    p.iter().zip(&bursts).map(|(a, b)| (a + b) as f32).collect()
}

/// SMPTE IMD pair: 60 Hz + 7 kHz at 4:1 amplitude, sum peaking at `peak_dbfs`.
pub fn smpte(peak_dbfs: f64, seconds: f64, fs: f64) -> Vec<f32> {
    two_tone(60.0, 4.0, 7000.0, 1.0, peak_dbfs, seconds, fs)
}

/// CCIF IMD pair: 19 + 20 kHz equal amplitude, sum peaking at `peak_dbfs`.
pub fn ccif(peak_dbfs: f64, seconds: f64, fs: f64) -> Vec<f32> {
    two_tone(19000.0, 1.0, 20000.0, 1.0, peak_dbfs, seconds, fs)
}

fn two_tone(f1: f64, a1: f64, f2: f64, a2: f64, peak_dbfs: f64, seconds: f64, fs: f64) -> Vec<f32> {
    let n = (seconds * fs).round() as usize;
    let mut x: Vec<f64> = (0..n)
        .map(|i| {
            let t = i as f64 / fs;
            a1 * (2.0 * PI * f1 * t).sin() + a2 * (2.0 * PI * f2 * t).sin()
        })
        .collect();
    normalize_peak(&mut x, peak_dbfs);
    to_f32(&x)
}

struct Biquad {
    b: [f64; 3],
    a: [f64; 2],
    z: [f64; 2],
}

impl Biquad {
    fn run(&mut self, x: f64) -> f64 {
        let y = self.b[0] * x + self.z[0];
        self.z[0] = self.b[1] * x - self.a[0] * y + self.z[1];
        self.z[1] = self.b[2] * x - self.a[1] * y;
        y
    }
}

/// Approximation of long-term average speech spectrum: pink noise through a 1st-order
/// high-pass at 100 Hz and a 2nd-order (Butterworth) low-pass at 4 kHz, at the given RMS.
pub fn ltass(rms_dbfs: f64, seconds: f64, fs: f64) -> Vec<f32> {
    let n = (seconds * fs) as usize;
    let k = (PI * 100.0 / fs).tan();
    let mut hp = Biquad { b: [1.0 / (1.0 + k), -1.0 / (1.0 + k), 0.0], a: [(k - 1.0) / (1.0 + k), 0.0], z: [0.0; 2] };
    let w = 2.0 * PI * 4000.0 / fs;
    let al = w.sin() / (2.0 * std::f64::consts::FRAC_1_SQRT_2);
    let a0 = 1.0 + al;
    let c = w.cos();
    let mut lp = Biquad {
        b: [(1.0 - c) / 2.0 / a0, (1.0 - c) / a0, (1.0 - c) / 2.0 / a0],
        a: [-2.0 * c / a0, (1.0 - al) / a0],
        z: [0.0; 2],
    };
    let mut y: Vec<f64> = pink(0.0, n, 31).iter().map(|&v| lp.run(hp.run(v))).collect();
    scale_rms(&mut y, rms_dbfs);
    to_f32(&y)
}
