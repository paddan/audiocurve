//! Diagnostic: sibilant-like bursts (multitone 5-8 kHz). Reports energy the kernel creates
//! outside the input band (products of clipping / fast gain changes), relative to in-band.
use audiocurve_dsp::{BandFitting, Handle, Params, CENTERS};
#[path = "thd.rs"] #[allow(dead_code)] mod thd;

fn goertzel_pow(x: &[f32], f: f64, fs: f64) -> f64 {
    let w = 2.0 * std::f64::consts::PI * f / fs;
    let c = 2.0 * w.cos();
    let (mut s1, mut s2) = (0.0f64, 0.0f64);
    for v in x { let s0 = *v as f64 + c * s1 - s2; s2 = s1; s1 = s0; }
    s1 * s1 + s2 * s2 - c * s1 * s2
}

fn band_energy(x: &[f32], lo: f64, hi: f64, step: f64, fs: f64) -> f64 {
    let mut f = lo; let mut e = 0.0;
    while f <= hi { e += goertzel_pow(x, f, fs); f += step; }
    e
}

fn main() {
    let left = [(125.,20.),(250.,25.),(500.,35.),(750.,40.),(1000.,55.),(1500.,65.),(2000.,70.),(3000.,80.),(4000.,85.),(6000.,95.),(8000.,105.)];
    let right = [(125.,15.),(250.,15.),(500.,20.),(750.,25.),(1000.,35.),(1500.,45.),(2000.,50.),(3000.,60.),(4000.,65.),(6000.,75.),(8000.,85.)];
    let fs = 48000.0;
    let n = (fs * 3.0) as usize;
    
    
    let tones: Vec<(f64, f64)> = (0..80).map(|i| (5003.0 + 37.0 * i as f64, i as f64 * 2.399)).collect();
    for &peak_db in &[-30.0, -20.0, -12.0, -6.0] {
        let norm = 10f64.powf(peak_db / 20.0) / (tones.len() as f64).sqrt() / 3.0;
        let sig: Vec<f32> = (0..n).map(|k| {
            let t = k as f64 / fs;
            let ph = t % 0.25; // 80 ms burst every 250 ms, 5 ms ramps
            let env = if ph < 0.005 { ph / 0.005 } else if ph < 0.075 { 1.0 } else if ph < 0.080 { (0.080 - ph) / 0.005 } else { 0.0 };
            let s: f64 = tones.iter().map(|(f, p)| (6.283185307 * f * t + p).sin()).sum();
            (norm * env * s) as f32
        }).collect();
        let mut row = format!("{peak_db:>5} dBFS:");
        for (lab, mode, s) in [("tal50", (5.0, 50.0), 0.5), ("mus50", (10.0, 200.0), 0.5), ("tal100", (5.0, 50.0), 1.0)] {
            let mut p = Params::unity();
            p.left = thd::fit(&left, s); p.right = thd::fit(&right, s);
            p.attack_ms = mode.0; p.release_ms = mode.1;
            let h = Handle::new(fs, 512).unwrap();
            h.set_params(&p);
            let (mut l, mut r) = (sig.clone(), sig.clone());
            for (a, b) in l.chunks_mut(512).zip(r.chunks_mut(512)) { unsafe { h.process(a, b) }; }
            let w = &l[n / 3..];
            let inb = band_energy(w, 5000.0, 8000.0, 4.0, fs);
            let low = band_energy(w, 200.0, 4000.0, 4.0, fs);
            let high = band_energy(w, 9000.0, 20000.0, 4.0, fs);
            let peak = w.iter().fold(0f32, |m, v| m.max(v.abs()));
            row += &format!("  {lab}: lo {:>6.1} hi {:>6.1} pk {:>5.1}", 10.0 * (low / inb).log10(), 10.0 * (high / inb).log10(), 20.0 * peak.log10());
        }
        println!("{row}");
    }
}
