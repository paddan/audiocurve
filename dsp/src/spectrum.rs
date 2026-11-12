//! Input/output spectrum for the UI: lock-free sample capture on the audio thread, FFT
//! analysis on the reader (UI) thread.

use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

pub const SPECTRUM_FFT: usize = 4096;
pub const SPECTRUM_POINTS: usize = 96;

const FLOOR_DB: f32 = -140.0;
const F_MIN: f64 = 20.0;
const F_MAX: f64 = 20000.0;
const FS_FRACTION_MAX: f64 = 0.45;
const SMOOTH_TAU_S: f64 = 0.3;
/// Half width of a band around its centre: 1/12 octave (1/6-octave bands).
const HALF_BAND_OCT: f64 = 1.0 / 12.0;
/// Equivalent noise bandwidth of the Hann window in bins; the summed bin power of a
/// sine is this much larger than the power of its peak bin.
const HANN_ENBW_BINS: f64 = 1.5;

/// Channel order of the capture rings and `Spectrum` rows.
const IN_L: usize = 0;
const IN_R: usize = 1;
const OUT_L: usize = 2;
const OUT_R: usize = 3;

/// `repr(C)` spectrum shared with Swift (`ac_spectrum`). `[ear][point]`, ear 0 = left.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spectrum {
    pub freq_hz: [f32; SPECTRUM_POINTS],
    pub in_db: [[f32; SPECTRUM_POINTS]; 2],
    pub out_db: [[f32; SPECTRUM_POINTS]; 2],
}

impl Default for Spectrum {
    fn default() -> Self {
        Self {
            freq_hz: [0.0; SPECTRUM_POINTS],
            in_db: [[FLOOR_DB; SPECTRUM_POINTS]; 2],
            out_db: [[FLOOR_DB; SPECTRUM_POINTS]; 2],
        }
    }
}

/// Audio-thread side: four rings of the latest samples. Display only, so a reader may
/// see a torn mix of old and new samples.
pub struct Capture {
    rings: [Vec<AtomicU32>; 4],
    /// Total frames written so far (monotonic); the ring position is `idx % SPECTRUM_FFT`.
    write: AtomicUsize,
}

impl Default for Capture {
    fn default() -> Self {
        Self::new()
    }
}

impl Capture {
    pub fn new() -> Self {
        Self {
            rings: std::array::from_fn(|_| (0..SPECTRUM_FFT).map(|_| AtomicU32::new(0)).collect()),
            write: AtomicUsize::new(0),
        }
    }

    /// Current write index. Audio thread only (single writer).
    pub fn position(&self) -> usize {
        self.write.load(Ordering::Relaxed)
    }

    fn put(&self, ch: usize, base: usize, x: &[f32]) {
        let ring = &self.rings[ch];
        for (i, &v) in x.iter().enumerate() {
            ring[base.wrapping_add(i) & (SPECTRUM_FFT - 1)].store(v.to_bits(), Ordering::Relaxed);
        }
    }

    /// Stores the chunk's input samples at `base` (from `position()`), before `publish`.
    pub fn put_in(&self, base: usize, l: &[f32], r: &[f32]) {
        self.put(IN_L, base, l);
        self.put(IN_R, base, r);
    }

    pub fn put_out(&self, base: usize, l: &[f32], r: &[f32]) {
        self.put(OUT_L, base, l);
        self.put(OUT_R, base, r);
    }

    /// Makes `frames` frames written at `base` visible to the reader.
    pub fn publish(&self, base: usize, frames: usize) {
        self.write.store(base.wrapping_add(frames), Ordering::Release);
    }
}

/// Reader-side state, owned by a `Mutex` in the handle so buffers are reused.
pub struct Reader {
    fs: f64,
    freqs: [f32; SPECTRUM_POINTS],
    /// Inclusive bin range per point.
    bins: [(usize, usize); SPECTRUM_POINTS],
    window: Vec<f64>,
    window_sum: f64,
    twiddle: Vec<(f64, f64)>,
    re: Vec<f64>,
    im: Vec<f64>,
    smoothed: [[f32; SPECTRUM_POINTS]; 4],
    prev_idx: [Option<usize>; 4],
}

impl Reader {
    pub fn new(fs: f64) -> Self {
        let fmax = F_MAX.min(FS_FRACTION_MAX * fs);
        let mut freqs = [0f32; SPECTRUM_POINTS];
        let mut bins = [(0usize, 0usize); SPECTRUM_POINTS];
        let bin_hz = fs / SPECTRUM_FFT as f64;
        let nyq_bin = SPECTRUM_FFT / 2;
        for i in 0..SPECTRUM_POINTS {
            let f = F_MIN * (fmax / F_MIN).powf(i as f64 / (SPECTRUM_POINTS - 1) as f64);
            freqs[i] = f as f32;
            let lo = (f * 2f64.powf(-HALF_BAND_OCT) / bin_hz).ceil() as usize;
            let hi = ((f * 2f64.powf(HALF_BAND_OCT) / bin_hz).floor() as usize).min(nyq_bin);
            bins[i] = if lo <= hi {
                (lo.max(1), hi.max(1))
            } else {
                let n = ((f / bin_hz).round() as usize).clamp(1, nyq_bin);
                (n, n)
            };
        }
        let n = SPECTRUM_FFT;
        // Periodic Hann.
        let window: Vec<f64> = (0..n)
            .map(|i| 0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / n as f64).cos())
            .collect();
        let window_sum = window.iter().sum();
        let twiddle = (0..n / 2)
            .map(|k| {
                let a = -2.0 * std::f64::consts::PI * k as f64 / n as f64;
                (a.cos(), a.sin())
            })
            .collect();
        Self {
            fs,
            freqs,
            bins,
            window,
            window_sum,
            twiddle,
            re: vec![0.0; n],
            im: vec![0.0; n],
            smoothed: [[FLOOR_DB; SPECTRUM_POINTS]; 4],
            prev_idx: [None; 4],
        }
    }

    /// In-place iterative radix-2 FFT of `re`/`im` (length a power of two).
    fn fft(&mut self) {
        let n = self.re.len();
        let mut j = 0;
        for i in 1..n {
            let mut bit = n >> 1;
            while j & bit != 0 {
                j ^= bit;
                bit >>= 1;
            }
            j |= bit;
            if i < j {
                self.re.swap(i, j);
                self.im.swap(i, j);
            }
        }
        let mut len = 2;
        while len <= n {
            let step = n / len;
            for start in (0..n).step_by(len) {
                for k in 0..len / 2 {
                    let (wr, wi) = self.twiddle[k * step];
                    let (a, b) = (start + k, start + k + len / 2);
                    let tr = self.re[b] * wr - self.im[b] * wi;
                    let ti = self.re[b] * wi + self.im[b] * wr;
                    self.re[b] = self.re[a] - tr;
                    self.im[b] = self.im[a] - ti;
                    self.re[a] += tr;
                    self.im[a] += ti;
                }
            }
            len <<= 1;
        }
    }

    /// Analyses one channel into `out` (dB, smoothed) from the capture's latest samples.
    fn channel(&mut self, cap: &Capture, ch: usize, idx: usize, out: &mut [f32; SPECTRUM_POINTS]) {
        let n = SPECTRUM_FFT;
        for i in 0..n {
            let pos = idx.wrapping_sub(n).wrapping_add(i) & (n - 1);
            let v = f32::from_bits(cap.rings[ch][pos].load(Ordering::Relaxed)) as f64;
            self.re[i] = if v.is_finite() { v * self.window[i] } else { 0.0 };
            self.im[i] = 0.0;
        }
        self.fft();
        let scale = 2.0 / self.window_sum;
        let mut raw = [FLOOR_DB; SPECTRUM_POINTS];
        for (p, &(lo, hi)) in self.bins.iter().enumerate() {
            let mut power = 0.0;
            for k in lo..=hi {
                let m = (self.re[k] * self.re[k] + self.im[k] * self.im[k]).sqrt() * scale;
                power += m * m;
            }
            // Band power, normalised so a sine reads its own level wherever it falls.
            raw[p] = ((10.0 * (power / HANN_ENBW_BINS).max(1e-30).log10()) as f32).max(FLOOR_DB);
        }
        let a = match self.prev_idx[ch] {
            None => 1.0,
            Some(prev) => {
                let dt = (idx.wrapping_sub(prev) as f64 / self.fs).clamp(0.0, 1.0);
                1.0 - (-dt / SMOOTH_TAU_S).exp()
            }
        } as f32;
        self.prev_idx[ch] = Some(idx);
        for p in 0..SPECTRUM_POINTS {
            let s = &mut self.smoothed[ch][p];
            *s += (raw[p] - *s) * a;
            out[p] = *s;
        }
    }

    pub fn read(&mut self, cap: &Capture, out: &mut Spectrum) {
        let idx = cap.write.load(Ordering::Acquire);
        out.freq_hz = self.freqs;
        let mut rows = [[FLOOR_DB; SPECTRUM_POINTS]; 4];
        for ch in [IN_L, IN_R, OUT_L, OUT_R] {
            self.channel(cap, ch, idx, &mut rows[ch]);
        }
        out.in_db = [rows[IN_L], rows[IN_R]];
        out.out_db = [rows[OUT_L], rows[OUT_R]];
    }
}
