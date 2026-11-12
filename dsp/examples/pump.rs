//! Diagnostic: how much does a steady 200 Hz tone get modulated by HF bursts?
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

fn fit(pts: &[(f64, f64)], s: f64) -> [BandFitting; 12] {
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

fn main() {
    let left = [(125.,20.),(250.,25.),(500.,35.),(750.,40.),(1000.,55.),(1500.,65.),(2000.,70.),(3000.,80.),(4000.,85.),(6000.,95.),(8000.,105.)];
    let right = [(125.,15.),(250.,15.),(500.,20.),(750.,25.),(1000.,35.),(1500.,45.),(2000.,50.),(3000.,60.),(4000.,65.),(6000.,75.),(8000.,85.)];
    let fs = 48000.0;
    let n = (fs * 4.0) as usize;
    // 200 Hz tone at -20 dBFS peak, plus 30 ms noise bursts (high-passed by differencing) every 250 ms.
    let mut seed = 1u32;
    let mut sig = vec![0f32; n];
    let mut prev = 0f32;
    for i in 0..n {
        let t = i as f64 / fs;
        let tone = 0.1 * (2.0 * std::f64::consts::PI * 200.0 * t).sin();
        seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
        let w = (seed >> 8) as f32 / (1 << 24) as f32 * 2.0 - 1.0;
        let hp = w - prev; prev = w;
        let burst = if (t % 0.25) < 0.03 { 0.15 * hp } else { 0.0 };
        sig[i] = tone as f32 + burst;
    }
    println!("{:<34} {:>10} {:>10}", "case", "mod L dB", "mod R dB");
    for &(name, s, atk, rel) in &[("speech s=50%", 0.5, 5.0, 50.0), ("music s=50%", 0.5, 10.0, 200.0), ("speech s=100%", 1.0, 5.0, 50.0), ("music s=100%", 1.0, 10.0, 200.0)] {
        for &(og, lab) in &[(0.0, "")] {
            let mut p = Params::unity();
            p.left = fit(&left, s); p.right = fit(&right, s);
            p.attack_ms = atk; p.release_ms = rel; p.output_gain_db = og; p.mpo_dbfs = -3.0;
            let h = Handle::new(fs, 512).unwrap();
            h.set_params(&p);
            let (mut l, mut r) = (sig.clone(), sig.clone());
            for (cl, cr) in l.chunks_mut(512).zip(r.chunks_mut(512)) { unsafe { h.process(cl, cr) }; }
            let m = |x: &[f32]| {
                // lock-in at 200 Hz, 10 ms one-pole, envelope over last 2 s
                let a = 1.0 - (-1.0f64 / (0.01 * fs)).exp();
                let (mut i_, mut q) = (0.0f64, 0.0f64);
                let (mut lo, mut hi) = (f64::MAX, f64::MIN);
                for (k, v) in x.iter().enumerate() {
                    let ph = 2.0 * std::f64::consts::PI * 200.0 * k as f64 / fs;
                    i_ += (*v as f64 * ph.sin() - i_) * a; q += (*v as f64 * ph.cos() - q) * a;
                    if k > n / 2 { let e = 20.0 * (i_.hypot(q)).log10(); lo = lo.min(e); hi = hi.max(e); }
                }
                hi - lo
            };
            println!("{:<34} {:>10.1} {:>10.1}", format!("{name} {lab}"), m(&l), m(&r));
        }
    }
}
