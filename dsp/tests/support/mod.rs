#![allow(dead_code)]
//! Shared test helpers.

pub fn sine(freq: f64, level_dbfs: f64, seconds: f64, fs: f64) -> Vec<f32> {
    let n = (seconds * fs).round() as usize;
    let amp = 10f64.powf(level_dbfs / 20.0);
    (0..n)
        .map(|i| (amp * (2.0 * std::f64::consts::PI * freq * i as f64 / fs).sin()) as f32)
        .collect()
}

/// 20·log10(RMS·√2) over the signal after `skip_seconds`.
pub fn level_dbfs(x: &[f32], skip_seconds: f64, fs: f64) -> f64 {
    let start = ((skip_seconds * fs) as usize).min(x.len());
    let s = &x[start..];
    if s.is_empty() {
        return -300.0;
    }
    let ms: f64 = s.iter().map(|&v| (v as f64) * (v as f64)).sum::<f64>() / s.len() as f64;
    20.0 * (ms.sqrt() * std::f64::consts::SQRT_2).max(1e-15).log10()
}

/// Deterministic xorshift64* noise in [-amplitude, amplitude].
pub fn noise(amplitude: f32, n: usize, seed: u64) -> Vec<f32> {
    let mut s = seed.max(1);
    (0..n)
        .map(|_| {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            let u = (s >> 40) as f32 / (1u64 << 24) as f32; // [0,1)
            (u * 2.0 - 1.0) * amplitude
        })
        .collect()
}

// ---- pumping measurement (shared with examples/pump.rs logic) ----
use audiocurve_dsp::{BandFitting, CENTERS};

pub const PUMP_LEFT: [(f64, f64); 11] = [(125.,20.),(250.,25.),(500.,35.),(750.,40.),(1000.,55.),(1500.,65.),(2000.,70.),(3000.,80.),(4000.,85.),(6000.,95.),(8000.,105.)];
pub const PUMP_RIGHT: [(f64, f64); 11] = [(125.,15.),(250.,15.),(500.,20.),(750.,25.),(1000.,35.),(1500.,45.),(2000.,50.),(3000.,60.),(4000.,65.),(6000.,75.),(8000.,85.)];

fn hl(pts: &[(f64, f64)], f: f64) -> f64 {
    if f <= pts[0].0 { return pts[0].1; }
    if f >= pts[pts.len() - 1].0 { return pts[pts.len() - 1].1; }
    for i in 1..pts.len() {
        if f <= pts[i].0 {
            let (lo, hi) = (pts[i - 1], pts[i]);
            let t = (f.ln() - lo.0.ln()) / (hi.0.ln() - lo.0.ln());
            return lo.1 + t * (hi.1 - lo.1);
        }
    }
    unreachable!()
}

pub fn pump_fit(pts: &[(f64, f64)], s: f64) -> [BandFitting; 12] {
    let mut out = [BandFitting::UNITY; 12];
    for (i, f) in CENTERS.iter().enumerate() {
        let h = hl(pts, *f);
        let corr = if *f <= 250.0 { 10.0 } else if *f == 500.0 { 5.0 } else { 0.0 };
        let gs = (0.6 * h - corr).max(0.0) * s;
        let gl = (0.25 * h - corr).max(0.0) * s;
        let den = 45.0 - (gs - gl);
        let cr = if den <= 0.0 { 10.0 } else { (45.0 / den).clamp(1.0, 10.0) };
        out[i] = BandFitting { gain_soft_db: gs, gain_loud_db: gl, compression_ratio: cr };
    }
    out
}

/// 200 Hz tone at -20 dBFS plus 30 ms HF noise bursts every 250 ms.
pub fn pump_signal(fs: f64, seconds: f64) -> Vec<f32> {
    let n = (fs * seconds) as usize;
    let mut seed = 1u32;
    let mut prev = 0f32;
    (0..n)
        .map(|i| {
            let t = i as f64 / fs;
            let tone = 0.1 * (2.0 * std::f64::consts::PI * 200.0 * t).sin();
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            let w = (seed >> 8) as f32 / (1 << 24) as f32 * 2.0 - 1.0;
            let hp = w - prev;
            prev = w;
            let burst = if (t % 0.25) < 0.03 { 0.15 * hp } else { 0.0 };
            tone as f32 + burst
        })
        .collect()
}

/// Swing (dB) of the 200 Hz lock-in envelope over the second half of `x`.
pub fn mod_200_db(x: &[f32], fs: f64) -> f64 {
    let a = 1.0 - (-1.0f64 / (0.01 * fs)).exp();
    let (mut i_, mut q) = (0.0f64, 0.0f64);
    let (mut lo, mut hi) = (f64::MAX, f64::MIN);
    for (k, v) in x.iter().enumerate() {
        let ph = 2.0 * std::f64::consts::PI * 200.0 * k as f64 / fs;
        i_ += (*v as f64 * ph.sin() - i_) * a;
        q += (*v as f64 * ph.cos() - q) * a;
        if k > x.len() / 2 {
            let e = 20.0 * i_.hypot(q).log10();
            lo = lo.min(e);
            hi = hi.max(e);
        }
    }
    hi - lo
}
