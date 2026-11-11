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
