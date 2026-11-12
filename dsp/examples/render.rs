//! Diagnostic: render a float32 stereo WAV through the kernel with the user's audiogram.
//! usage: render <in.wav> <out.wav> <strength 0..1> <full|linear> [gain_db] [output_gain_db] [limiter 1|0]
use audiocurve_dsp::{BandFitting, Handle, Params};
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
            let d = &b[i + 8..i + 8 + len];
            let s: Vec<f32> = d.chunks_exact(4).map(|c| f32::from_le_bytes(c.try_into().unwrap())).collect();
            let l = s.iter().step_by(ch).copied().collect();
            let r = s.iter().skip(1).step_by(ch).copied().collect();
            return (fs, l, r);
        }
        i += 8 + len + (len & 1);
    }
}

fn write_wav(p: &str, fs: f64, l: &[f32], r: &[f32]) {
    let n = l.len();
    let mut b = Vec::with_capacity(44 + n * 8);
    let data_len = (n * 8) as u32;
    b.extend(b"RIFF"); b.extend((36 + data_len).to_le_bytes()); b.extend(b"WAVEfmt ");
    b.extend(16u32.to_le_bytes()); b.extend(3u16.to_le_bytes()); b.extend(2u16.to_le_bytes());
    b.extend((fs as u32).to_le_bytes()); b.extend((fs as u32 * 8).to_le_bytes()); b.extend(8u16.to_le_bytes()); b.extend(32u16.to_le_bytes());
    b.extend(b"data"); b.extend(data_len.to_le_bytes());
    for k in 0..n { b.extend(l[k].to_le_bytes()); b.extend(r[k].to_le_bytes()); }
    std::fs::write(p, b).unwrap();
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let (fs, mut l, mut r) = read_wav(&a[1]);
    // Excerpt 0:30-1:15, optional pre-scale (dB) so a linear reference never reaches the ceiling.
    let (s0, s1) = ((30.0 * fs) as usize, (75.0 * fs) as usize);
    let pre = 10f32.powf(a.get(5).map(|v| v.parse().unwrap()).unwrap_or(0.0f32) / 20.0);
    l = l[s0..s1].iter().map(|v| v * pre).collect();
    r = r[s0..s1].iter().map(|v| v * pre).collect();
    let s: f64 = a[3].parse().unwrap();
    let left = [(125.,20.),(250.,25.),(500.,35.),(750.,40.),(1000.,55.),(1500.,65.),(2000.,70.),(3000.,80.),(4000.,85.),(6000.,95.),(8000.,105.)];
    let right = [(125.,15.),(250.,15.),(500.,20.),(750.,25.),(1000.,35.),(1500.,45.),(2000.,50.),(3000.,60.),(4000.,65.),(6000.,75.),(8000.,85.)];
    let mut p = Params::unity();
    p.left = thd::fit(&left, s); p.right = thd::fit(&right, s);
    p.left[11].gain_soft_db -= 20.0 * s; p.left[11].gain_loud_db -= 20.0 * s; // user's -20 trim at 12k
    p.right[11].gain_soft_db -= 20.0 * s; p.right[11].gain_loud_db -= 20.0 * s;
    if a[4] == "linear" {
        for b in p.left.iter_mut().chain(p.right.iter_mut()) {
            *b = BandFitting { gain_soft_db: b.gain_loud_db, gain_loud_db: b.gain_loud_db, compression_ratio: 1.0 };
        }
    }

    p.attack_ms = 10.0; p.release_ms = 200.0; // Musik
    p.output_gain_db = a.get(6).map(|v| v.parse().unwrap()).unwrap_or(0.0);
    p.limiter_enabled = a.get(7).map(|v| v.parse().unwrap()).unwrap_or(1.0);
    let h = Handle::new(fs, 512).unwrap();
    h.set_params(&p);
    for (cl, cr) in l.chunks_mut(512).zip(r.chunks_mut(512)) { unsafe { h.process(cl, cr) }; }
    let pk = l.iter().chain(r.iter()).fold(0f32, |m, v| m.max(v.abs()));
    let rms = (l.iter().chain(r.iter()).map(|v| (*v as f64).powi(2)).sum::<f64>() / (2 * l.len()) as f64).sqrt(); // both ears: keep the L/R balance
    println!("{}: peak {:.1} dBFS, rms {:.1} dBFS", a[2], 20.0 * pk.log10(), 20.0 * rms.log10());
    // Loudness-match: normalise to -20 dBFS RMS (float WAV, so no clipping).
    let k = (10f64.powf(-20.0 / 20.0) / rms) as f32;
    for v in l.iter_mut().chain(r.iter_mut()) { *v *= k; }
    write_wav(&a[2], fs, &l, &r);
}
