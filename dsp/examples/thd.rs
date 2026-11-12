//! Diagnostic: THD+N of single sines through the kernel with the user's audiogram.
use audiocurve_dsp::{BandFitting, Handle, Params, CENTERS};

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

pub fn fit(pts: &[(f64, f64)], s: f64) -> [BandFitting; 12] {
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

/// Residual (everything except the fitted sine at f) relative to the sine, in dB.
fn thd_n(x: &[f32], f: f64, fs: f64) -> f64 {
    let w = 2.0 * std::f64::consts::PI * f / fs;
    let (mut a, mut b) = (0.0, 0.0);
    for (k, v) in x.iter().enumerate() { a += *v as f64 * (w * k as f64).sin(); b += *v as f64 * (w * k as f64).cos(); }
    let n = x.len() as f64; a *= 2.0 / n; b *= 2.0 / n;
    let (mut ps, mut pr) = (0.0, 0.0);
    for (k, v) in x.iter().enumerate() {
        let s = a * (w * k as f64).sin() + b * (w * k as f64).cos();
        ps += s * s; pr += (*v as f64 - s).powi(2);
    }
    10.0 * (pr / ps).log10()
}

fn main() {
    let left = [(125.,20.),(250.,25.),(500.,35.),(750.,40.),(1000.,55.),(1500.,65.),(2000.,70.),(3000.,80.),(4000.,85.),(6000.,95.),(8000.,105.)];
    let right = [(125.,15.),(250.,15.),(500.,20.),(750.,25.),(1000.,35.),(1500.,45.),(2000.,50.),(3000.,60.),(4000.,65.),(6000.,75.),(8000.,85.)];
    let fs = 48000.0;
    let mode = std::env::args().nth(1).unwrap_or("speech".into());
    let (atk, rel) = if mode == "music" { (10.0, 200.0) } else { (5.0, 50.0) };
    println!("THD+N dB (L / R), mode {mode}, strength 50%");
    print!("{:>8}", "f \\ in");
    let levels = [-40.0, -30.0, -20.0, -10.0, -3.0];
    for l in levels { print!("{:>16}", format!("{l} dBFS")); }
    println!();
    for &f in &[200.0, 1000.0, 2500.0, 4000.0, 6000.0, 8000.0, 10000.0, 14000.0] {
        print!("{:>8}", f);
        for &lv in &levels {
            let mut p = Params::unity();
            p.left = fit(&left, 0.5); p.right = fit(&right, 0.5);
            p.attack_ms = atk; p.release_ms = rel;
            let h = Handle::new(fs, 512).unwrap();
            h.set_params(&p);
            let n = (fs * 2.0) as usize;
            let amp = 10f64.powf(lv / 20.0);
            let sig: Vec<f32> = (0..n).map(|k| (amp * (2.0 * std::f64::consts::PI * f * k as f64 / fs).sin()) as f32).collect();
            let (mut l, mut r) = (sig.clone(), sig);
            for (a, b) in l.chunks_mut(512).zip(r.chunks_mut(512)) { unsafe { h.process(a, b) }; }
            // whole number of cycles over the last second
            let start = n / 2;
            let cyc = ((n - start) as f64 * f / fs).floor();
            let len = (cyc * fs / f).round() as usize;
            print!("{:>16}", format!("{:.0} / {:.0}", thd_n(&l[start..start + len], f, fs), thd_n(&r[start..start + len], f, fs)));
        }
        println!();
    }
}
