//! Diagnostic: how much does the safety stage (band budget + limiter) remove on a track?
//! usage: limstat <in48.wav> <strength>
use audiocurve_dsp::{Handle, Params};
#[path = "thd.rs"] #[allow(dead_code)] mod thd;

fn read_wav(p: &str) -> (f64, Vec<f32>, Vec<f32>) {
    let b = std::fs::read(p).unwrap();
    let u32le = |i: usize| u32::from_le_bytes(b[i..i + 4].try_into().unwrap());
    let (mut i, mut fs, mut ch) = (12, 0.0, 0);
    loop {
        let id = &b[i..i + 4];
        let len = u32le(i + 4) as usize;
        if id == b"fmt " { ch = u16::from_le_bytes(b[i + 10..i + 12].try_into().unwrap()) as usize; fs = u32le(i + 12) as f64; }
        if id == b"data" {
            let s: Vec<f32> = b[i + 8..i + 8 + len].chunks_exact(4).map(|c| f32::from_le_bytes(c.try_into().unwrap())).collect();
            return (fs, s.iter().step_by(ch).copied().collect(), s.iter().skip(1).step_by(ch).copied().collect());
        }
        i += 8 + len + (len & 1);
    }
}

fn render(fs: f64, l: &[f32], r: &[f32], p: &Params) -> (Vec<f32>, Vec<f32>) {
    let (mut l, mut r) = (l.to_vec(), r.to_vec());
    let h = Handle::new(fs, 512).unwrap();
    h.set_params(p);
    for (cl, cr) in l.chunks_mut(512).zip(r.chunks_mut(512)) { unsafe { h.process(cl, cr) }; }
    (l, r)
}

// Energy in a band [lo,hi) per frame via a crude 2nd-order bandpass (RBJ, constant peak gain).
fn band_frames(x: &[f32], fs: f64, f0: f64, frame: usize) -> Vec<f64> {
    let q = 1.4; let w = 2.0 * std::f64::consts::PI * f0 / fs; let al = w.sin() / (2.0 * q);
    let (b0, b2, a0, a1, a2) = (al, -al, 1.0 + al, -2.0 * w.cos(), 1.0 - al);
    let (mut x1, mut x2, mut y1, mut y2) = (0.0, 0.0, 0.0, 0.0);
    let mut out = Vec::new(); let mut acc = 0.0;
    for (k, &v) in x.iter().enumerate() {
        let v = v as f64;
        let y = (b0 * v + b2 * x2 - a1 * y1 - a2 * y2) / a0;
        x2 = x1; x1 = v; y2 = y1; y1 = y;
        acc += y * y;
        if (k + 1) % frame == 0 { out.push(acc / frame as f64); acc = 0.0; }
    }
    out
}

fn pct(mut v: Vec<f64>, q: f64) -> f64 { v.sort_by(|a, b| a.partial_cmp(b).unwrap()); v[((v.len() - 1) as f64 * q) as usize] }

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let (fs, l, r) = read_wav(&a[1]);
    let (s0, s1) = ((30.0 * fs) as usize, (75.0 * fs) as usize);
    let (l, r) = (&l[s0..s1], &r[s0..s1]);
    let s: f64 = a[2].parse().unwrap();
    let left = [(125.,20.),(250.,25.),(500.,35.),(750.,40.),(1000.,55.),(1500.,65.),(2000.,70.),(3000.,80.),(4000.,85.),(6000.,95.),(8000.,105.)];
    let right = [(125.,15.),(250.,15.),(500.,20.),(750.,25.),(1000.,35.),(1500.,45.),(2000.,50.),(3000.,60.),(4000.,65.),(6000.,75.),(8000.,85.)];
    let pre: f64 = a.get(3).map(|v| v.parse().unwrap()).unwrap_or(0.0);
    let k = 10f32.powf(pre as f32 / 20.0);
    let (l, r): (Vec<f32>, Vec<f32>) = (l.iter().map(|v| v * k).collect(), r.iter().map(|v| v * k).collect());
    let mut p = Params::unity();
    p.left = thd::fit(&left, s); p.right = thd::fit(&right, s);
    p.attack_ms = 10.0; p.release_ms = 200.0;
    let mut off = p.clone(); off.limiter_enabled = 0.0;
    let (al, ar) = render(fs, &l, &r, &p);
    let (bl, br) = render(fs, &l, &r, &off);
    let ceil = 10f32.powf(p.mpo_dbfs as f32 / 20.0);
    for (name, b) in [("L", &bl), ("R", &br)] {
        let over = b.iter().filter(|v| v.abs() > ceil).count() as f64 / b.len() as f64 * 100.0;
        let pk = b.iter().fold(0f32, |m, v| m.max(v.abs()));
        println!("{name} limiter off: peak {:.1} dBFS, {:.2}% samples over Tak {:.0} dBFS", 20.0 * pk.log10(), over, p.mpo_dbfs);
    }
    let frame = (0.05 * fs) as usize;
    println!("reduction on vs off (dB), 50 ms frames: median / p95   [L | R]");
    for f0 in [125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0] {
        let mut row = format!("{:>6} Hz:", f0);
        for (x, y) in [(&al, &bl), (&ar, &br)] {
            let (ea, eb) = (band_frames(x, fs, f0, frame), band_frames(y, fs, f0, frame));
            let d: Vec<f64> = ea.iter().zip(&eb).filter(|(_, b)| **b > 1e-9).map(|(a, b)| 10.0 * (b / a.max(1e-20)).log10()).collect();
            row += &format!("  {:5.1} / {:5.1}", pct(d.clone(), 0.5), pct(d, 0.95));
        }
        println!("{row}");
    }
}
