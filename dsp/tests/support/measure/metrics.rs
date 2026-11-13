//! The measurements themselves. Everything runs at 48 kHz through `Handle` in 512-frame
//! blocks (except `gainmod_hf`, which drives the filterbank and compressors directly) and
//! discards the first second; the distortion tones settle `TONE_SETTLE_S` first.

use super::{params, prescription, signals, support, Audiogram, Ear, Mode};
use audiocurve_dsp::compressor::BandCompressor;
use audiocurve_dsp::{BandBuffers, Filterbank, Handle, Params, CENTERS};
use std::f64::consts::PI;
use std::path::Path;

pub const FS: f64 = 48_000.0;
const BLOCK: usize = 512;
const SKIP: usize = 48_000;
/// THD+N and IMD tones run this long before their 1 s analysis window, so every detector
/// (slowest release 1.5 s) and the budget release have settled after the onset.
const TONE_SETTLE_S: f64 = 8.0;
const TONE_S: f64 = TONE_SETTLE_S + 1.0;
const MPO_DBFS: f64 = -3.0;
/// Distortion below this is f32 noise floor; clamped so the regression gate is stable.
const DB_FLOOR: f64 = -120.0;

/// Optional WAV dump of a metric's input/output pair (left ear, 32-bit float).
struct Wav<'a> {
    dir: Option<&'a Path>,
    tag: String,
}

impl Wav<'_> {
    fn save(&self, metric: &str, input: &[f32], output: &[f32]) {
        let Some(dir) = self.dir else { return };
        std::fs::create_dir_all(dir).unwrap();
        for (suffix, x) in [("in", input), ("out", output)] {
            write_wav(&dir.join(format!("{}_{metric}_{suffix}.wav", self.tag)), x);
        }
    }
}

fn write_wav(path: &Path, x: &[f32]) {
    let data = (x.len() * 4) as u32;
    let mut b = Vec::with_capacity(44 + x.len() * 4);
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + data).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&3u16.to_le_bytes()); // IEEE float
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&(FS as u32).to_le_bytes());
    b.extend_from_slice(&(FS as u32 * 4).to_le_bytes());
    b.extend_from_slice(&4u16.to_le_bytes());
    b.extend_from_slice(&32u16.to_le_bytes());
    b.extend_from_slice(b"data");
    b.extend_from_slice(&data.to_le_bytes());
    for v in x {
        b.extend_from_slice(&v.to_le_bytes());
    }
    std::fs::write(path, b).unwrap();
}

struct Rendered {
    left: Vec<f32>,
    right: Vec<f32>,
}

/// Same signal on both ears through a fresh kernel.
fn render(p: &Params, sig: &[f32]) -> Rendered {
    let h = Handle::new(FS, BLOCK).unwrap();
    h.set_params(p);
    let (mut left, mut right) = (sig.to_vec(), sig.to_vec());
    for (l, r) in left.chunks_mut(BLOCK).zip(right.chunks_mut(BLOCK)) {
        unsafe { h.process(l, r) };
    }
    Rendered { left, right }
}

/// All metrics for one audiogram and mode: `(metric name, value)`.
pub fn measure(aname: &str, a: &Audiogram, mode: Mode, wav_dir: Option<&Path>) -> Vec<(String, f64)> {
    let wav = Wav { dir: wav_dir, tag: format!("{aname}_{}", mode.name()) };
    let p = params(a, mode);
    let mut out = Vec::new();
    if mode != Mode::Auto {
        thdn(&p, &wav, &mut out);
        imd(&p, &wav, &mut out);
        out.push(("gainmod_hf".into(), gainmod_hf(a, mode, &wav)));
    }
    let dense = signals::dense(6.0, FS);
    let r = render(&p, &dense);
    safety(&p, &wav, &dense, &mut out);
    if mode != Mode::Auto {
        out.push(("isp_over_db".into(), isp_over_db(&p, &r, &wav)));
        tilt(&p, &wav, &mut out);
    }
    out
}

fn thdn(p: &Params, wav: &Wav, out: &mut Vec<(String, f64)>) {
    for f in [250.0, 1000.0, 4000.0] {
        for (tag, level) in [("m40", -40.0), ("m20", -20.0), ("m6", -6.0)] {
            let sig = signals::sine(f, level, 0.0, TONE_S, FS);
            let r = render(p, &sig);
            let n = r.left.len();
            // Last second holds a whole number of cycles for these frequencies.
            let v = [&r.left, &r.right]
                .iter()
                .map(|ch| support::thd_n_db(&ch[n - SKIP..], f, FS))
                .fold(f64::MIN, f64::max);
            wav.save(&format!("thdn_{}_{tag}", f as u32), &sig, &r.left);
            out.push((format!("thdn_{}_{tag}", f as u32), v.max(DB_FLOOR)));
        }
    }
}

/// Power (amplitude squared) of the component at integer frequency `f` over a signal that
/// holds a whole number of its cycles.
fn component_power(x: &[f32], f: f64) -> f64 {
    let w = 2.0 * PI * f / FS;
    let (mut s, mut c) = (0.0, 0.0);
    for (k, v) in x.iter().enumerate() {
        let (sn, cs) = (w * k as f64).sin_cos();
        s += *v as f64 * sn;
        c += *v as f64 * cs;
    }
    let n = x.len() as f64;
    (2.0 * s / n).powi(2) + (2.0 * c / n).powi(2)
}

fn imd(p: &Params, wav: &Wav, out: &mut Vec<(String, f64)>) {
    for (tag, level) in [("m20", -20.0), ("m6", -6.0)] {
        // SMPTE: sidebands of 7 kHz at +-n*60 Hz relative to the 7 kHz component.
        let sig = signals::smpte(level, TONE_S, FS);
        let r = render(p, &sig);
        let n = r.left.len();
        let v = [&r.left, &r.right]
            .iter()
            .map(|ch| {
                let x = &ch[n - SKIP..];
                let sb: f64 = (1..=4)
                    .flat_map(|k| [7000.0 - 60.0 * k as f64, 7000.0 + 60.0 * k as f64])
                    .map(|f| component_power(x, f))
                    .sum();
                10.0 * (sb / component_power(x, 7000.0)).log10()
            })
            .fold(f64::MIN, f64::max);
        wav.save(&format!("imd_smpte_{tag}"), &sig, &r.left);
        out.push((format!("imd_smpte_{tag}"), v.max(DB_FLOOR)));

        // CCIF: products at 1 kHz, 18 kHz and 21 kHz relative to the two tones.
        let sig = signals::ccif(level, TONE_S, FS);
        let r = render(p, &sig);
        let n = r.left.len();
        let v = [&r.left, &r.right]
            .iter()
            .map(|ch| {
                let x = &ch[n - SKIP..];
                let prod: f64 = [1000.0, 18000.0, 21000.0].iter().map(|&f| component_power(x, f)).sum();
                let tones = component_power(x, 19000.0) + component_power(x, 20000.0);
                10.0 * (prod / tones).log10()
            })
            .fold(f64::MIN, f64::max);
        wav.save(&format!("imd_ccif_{tag}"), &sig, &r.left);
        out.push((format!("imd_ccif_{tag}"), v.max(DB_FLOOR)));
    }
}

/// Mean over bands above 2 kHz of the standard deviation (dB) of the WDRC gain on
/// modulated pink noise (left ear fitting, kernel attack/release for the mode).
fn gainmod_hf(a: &Audiogram, mode: Mode, wav: &Wav) -> f64 {
    let (speech_fits, music_fits) = prescription::pair(a, Ear::Left, 0.0);
    let p = params(a, mode);
    // Kernel time constants for the weight (Auto is measured at the Music values).
    let timing = audiocurve_dsp::compressor::blend_timing(p.speech_weight);
    let sig = signals::modulated_pink(-20.0, 4.0, FS);
    let n = sig.len();
    let mut bank = Filterbank::new(FS);
    let mut bufs = BandBuffers::new(BLOCK);
    let mut comps: Vec<BandCompressor> = CENTERS.iter().map(|_| BandCompressor::new(FS)).collect();
    let mut gains = vec![vec![0f32; n]; CENTERS.len()];
    let mut levels = vec![0f32; BLOCK];
    let mut gblock = vec![0f32; BLOCK];
    let mut mix = vec![0f32; n];
    for (bi, block) in sig.chunks(BLOCK).enumerate() {
        let (s, m) = (bi * BLOCK, block.len());
        bank.split(block, &mut bufs);
        for (i, comp) in comps.iter_mut().enumerate() {
            comp.analyze(&bufs.bands[i][..m], &speech_fits[i], &music_fits[i], p.speech_weight, &timing, &mut gblock[..m], &mut levels[..m]);
            gains[i][s..s + m].copy_from_slice(&gblock[..m]);
            for t in 0..m {
                mix[s + t] += bufs.bands[i][t] * 10f32.powf(gblock[t] / 20.0);
            }
        }
    }
    wav.save("gainmod_hf", &sig, &mix);
    let tail = n - 3 * FS as usize;
    let stds: Vec<f64> = CENTERS
        .iter()
        .enumerate()
        .filter(|&(_, &c)| c > 2000.0)
        .map(|(i, _)| {
            let g = &gains[i][tail..];
            let mean = g.iter().map(|&v| v as f64).sum::<f64>() / g.len() as f64;
            (g.iter().map(|&v| (v as f64 - mean).powi(2)).sum::<f64>() / g.len() as f64).sqrt()
        })
        .collect();
    stds.iter().sum::<f64>() / stds.len() as f64
}

/// (active %, mean dB, max dB) of the safety-stage reduction on `input`: per-block energy
/// ratio between the signal rendered with the safety stage off and on (`on` = render with `p`).
fn safety_stats(p: &Params, on: &Rendered, input: &[f32]) -> (f64, f64, f64) {
    let mut off_p = *p;
    off_p.limiter_enabled = 0.0;
    off_p.mpo_dbfs = -1.0;
    let off = render(&off_p, input);
    let energy = |x: &[f32]| x.iter().map(|&v| (v as f64) * (v as f64)).sum::<f64>();
    let red: Vec<f64> = off.left[SKIP..]
        .chunks(BLOCK)
        .zip(on.left[SKIP..].chunks(BLOCK))
        .map(|(a, b)| {
            let (ea, eb) = (energy(a), energy(b));
            if ea < 1e-12 || eb < 1e-12 { 0.0 } else { (10.0 * (ea / eb).log10()).max(0.0) }
        })
        .collect();
    let n = red.len() as f64;
    (
        100.0 * red.iter().filter(|&&v| v > 0.1).count() as f64 / n,
        red.iter().sum::<f64>() / n,
        red.iter().fold(0.0, |a, &v| a.max(v)),
    )
}

/// "Auto level" operating point: the highest output gain (0 down to -40 dB, 1 dB steps) at
/// which the bed alone (no bursts) leaves the safety stage idle (mean reduction <= 0.1 dB).
/// Coarse 6 dB steps first, then a 1 dB refinement.
fn auto_level_gain(p: &Params) -> f64 {
    let bed = signals::to_f32(&signals::pink(signals::DENSE_BED_DBFS, 2 * FS as usize, 5));
    let idle = |g: f64| {
        let mut q = *p;
        q.output_gain_db = g;
        safety_stats(&q, &render(&q, &bed), &bed).1 <= 0.1
    };
    let mut coarse = 0.0;
    while coarse > -40.0 && !idle(coarse) {
        coarse = (coarse - 6.0).max(-40.0);
    }
    (1..=5).map(|k| coarse + (6 - k) as f64).filter(|&g| g <= 0.0 && g > coarse).find(|&g| idle(g)).unwrap_or(coarse)
}

/// Safety-stage activity on the dense signal at the auto-level output gain.
fn safety(p: &Params, wav: &Wav, input: &[f32], out: &mut Vec<(String, f64)>) {
    let mut q = *p;
    q.output_gain_db = auto_level_gain(p);
    let on = render(&q, input);
    let (pct, mean, max) = safety_stats(&q, &on, input);
    out.push(("safety_output_gain_db".into(), q.output_gain_db));
    out.push(("safety_active_pct".into(), pct));
    out.push(("safety_mean_db".into(), mean));
    out.push(("safety_max_db".into(), max));
    wav.save("safety", input, &on.left);
}

/// Highest 4x-oversampled true peak (linear) of `x` after the first second (also used by
/// `tests/limiter.rs`).
pub fn true_peak(x: &[f32]) -> f64 {
    const HALF: isize = 32; // 64 taps per phase
    const BETA: f64 = 8.0;
    let i0 = |v: f64| {
        let (mut sum, mut term) = (1.0, 1.0);
        for k in 1..60 {
            term *= (v / (2.0 * k as f64)).powi(2);
            sum += term;
        }
        sum
    };
    let taps: Vec<Vec<f64>> = (1..4)
        .map(|ph| {
            let d = ph as f64 / 4.0;
            (-HALF + 1..=HALF)
                .map(|j| {
                    let t = j as f64 - d;
                    let sinc = if t == 0.0 { 1.0 } else { (PI * t).sin() / (PI * t) };
                    let r = t / HALF as f64;
                    sinc * i0(BETA * (1.0 - r * r).max(0.0).sqrt()) / i0(BETA)
                })
                .collect()
        })
        .collect();
    let mut peak = 0f64;
    for n in SKIP..x.len() - HALF as usize - 1 {
        peak = peak.max(x[n].abs() as f64);
        for h in &taps {
            let y: f64 = h.iter().enumerate().map(|(i, &c)| c * x[n + i + 1 - HALF as usize] as f64).sum();
            peak = peak.max(y.abs());
        }
    }
    peak
}

/// Worst true peak over both ears, on the dense signal and on a full-scale fs/4 sine at
/// 45 degrees (worst-case inter-sample peak), in dB above the MPO ceiling.
fn isp_over_db(p: &Params, dense: &Rendered, wav: &Wav) -> f64 {
    let sig = signals::sine(FS / 4.0, 0.0, PI / 4.0, 2.0, FS);
    let s = render(p, &sig);
    wav.save("isp", &sig, &s.left);
    let peak = [&dense.left, &dense.right, &s.left, &s.right]
        .iter()
        .map(|x| true_peak(x))
        .fold(0f64, f64::max);
    20.0 * peak.log10() - MPO_DBFS
}

fn fft(re: &mut [f64], im: &mut [f64]) {
    let n = re.len();
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let ang = -2.0 * PI / len as f64;
        for s in (0..n).step_by(len) {
            for k in 0..len / 2 {
                let (wr, wi) = ((ang * k as f64).cos(), (ang * k as f64).sin());
                let (a, b) = (s + k, s + k + len / 2);
                let (tr, ti) = (re[b] * wr - im[b] * wi, re[b] * wi + im[b] * wr);
                re[b] = re[a] - tr;
                im[b] = im[a] - ti;
                re[a] += tr;
                im[a] += ti;
            }
        }
        len <<= 1;
    }
}

/// Welch power spectrum (Hann, 50% overlap), bins 0..=n/2.
fn welch(x: &[f32], n: usize) -> Vec<f64> {
    let win: Vec<f64> = (0..n).map(|i| 0.5 - 0.5 * (2.0 * PI * i as f64 / n as f64).cos()).collect();
    let mut acc = vec![0f64; n / 2 + 1];
    let mut count = 0;
    let mut s = 0;
    while s + n <= x.len() {
        let mut re: Vec<f64> = (0..n).map(|i| x[s + i] as f64 * win[i]).collect();
        let mut im = vec![0f64; n];
        fft(&mut re, &mut im);
        for k in 0..=n / 2 {
            acc[k] += re[k] * re[k] + im[k] * im[k];
        }
        count += 1;
        s += n / 2;
    }
    acc.iter().map(|v| v / count as f64).collect()
}

/// Spectral tilt of the processing (output minus input spectrum) on speech-shaped noise.
fn tilt(p: &Params, wav: &Wav, out: &mut Vec<(String, f64)>) {
    const N: usize = 8192;
    let sig = signals::ltass(-20.0, 6.0, FS);
    let r = render(p, &sig);
    wav.save("tilt", &sig, &r.left);
    let (pi, po) = (welch(&sig[SKIP..], N), welch(&r.left[SKIP..], N));
    let bin_hz = FS / N as f64;
    let diff = |k: usize| 10.0 * (po[k] / pi[k]).log10();
    let in_range = |lo: f64, hi: f64| (1..N / 2).filter(move |&k| (lo..=hi).contains(&(k as f64 * bin_hz)));
    // Least-squares slope of the dB difference against log2(frequency).
    let pts: Vec<(f64, f64)> = in_range(250.0, 6000.0).map(|k| ((k as f64 * bin_hz).log2(), diff(k))).collect();
    let n = pts.len() as f64;
    let (mx, my) = (pts.iter().map(|q| q.0).sum::<f64>() / n, pts.iter().map(|q| q.1).sum::<f64>() / n);
    let slope = pts.iter().map(|q| (q.0 - mx) * (q.1 - my)).sum::<f64>() / pts.iter().map(|q| (q.0 - mx).powi(2)).sum::<f64>();
    let mean = |lo: f64, hi: f64| {
        let v: Vec<f64> = in_range(lo, hi).map(diff).collect();
        v.iter().sum::<f64>() / v.len() as f64
    };
    out.push(("tilt_db_per_oct".into(), slope));
    out.push(("lowhigh_diff_db".into(), mean(2000.0, 4000.0) - mean(250.0, 500.0)));
}
