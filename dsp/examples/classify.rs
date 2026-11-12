//! Diagnostic: run the speech/music classifier over a float32 stereo WAV, print once a second.
//! usage: classify <in.wav> [start_s] [len_s]
use audiocurve_dsp::classifier::Classifier;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let b = std::fs::read(&a[1]).unwrap();
    let u32le = |i: usize| u32::from_le_bytes(b[i..i + 4].try_into().unwrap());
    let (mut i, mut fs, mut ch) = (12, 0.0, 0);
    let (l, r) = loop {
        let id = &b[i..i + 4];
        let len = u32le(i + 4) as usize;
        if id == b"fmt " { ch = u16::from_le_bytes(b[i + 10..i + 12].try_into().unwrap()) as usize; fs = u32le(i + 12) as f64; }
        if id == b"data" {
            let s: Vec<f32> = b[i + 8..i + 8 + len].chunks_exact(4).map(|c| f32::from_le_bytes(c.try_into().unwrap())).collect();
            break (s.iter().step_by(ch).copied().collect::<Vec<_>>(), s.iter().skip(1).step_by(ch).copied().collect::<Vec<_>>());
        }
        i += 8 + len + (len & 1);
    };
    let s0 = (a.get(2).map(|v| v.parse().unwrap()).unwrap_or(0.0) * fs) as usize;
    let n = a.get(3).map(|v| (v.parse::<f64>().unwrap() * fs) as usize).unwrap_or(l.len() - s0).min(l.len() - s0);
    let mut c = Classifier::new(fs);
    let sec = fs as usize;
    let mut line = String::new();
    for (k, (cl, cr)) in l[s0..s0 + n].chunks(512).zip(r[s0..s0 + n].chunks(512)).enumerate() {
        c.process(cl, cr);
        if (k + 1) * 512 / sec != k * 512 / sec {
            line += &format!("{:.2}/{:.2} ", c.score(), c.speech());
        }
    }
    println!("{line}");
}
