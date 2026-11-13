//! Lookahead true-peak limiter: the hearing-safety backstop (MPO).
//!
//! Per channel (unlinked), for the sample `n = t - TP_TAPS_PER_PHASE/2`:
//! - true peak = max of `|x[n]|` and the three 4x-interpolated values after it
//!   (Kaiser-windowed sinc, `TP_TAPS_PER_PHASE` taps per phase);
//! - `need` = soft-knee gain mapping that peak onto `c * 10^(-TP_GUARD_DB/20)`;
//! - `h` = minimum of `need` over the last `L + 1` values (monotonic deque), `L` = lookahead;
//! - `gb` = moving average of `h` over the last `L` values: a smooth attack ramp spread over
//!   the whole lookahead, which still reaches every peak's `need` before that peak is output;
//! - `g` follows `gb` down immediately and up with a one-pole release (`RELEASE_S`).
//!
//! Output = input delayed by `L + TP_TAPS_PER_PHASE/2` times `g`, then a hard clamp to +-c, so
//! the ceiling is a guarantee. The guard keeps the clamp idle and covers the error of the short
//! interpolator.

const LOOKAHEAD_S: f64 = 0.005;
const RELEASE_S: f64 = 0.150;
const KNEE_DB: f64 = 3.0;
/// Target margin below the ceiling for the (interpolated) true peak.
const TP_GUARD_DB: f64 = 0.2;
const OVERSAMPLE: usize = 4;
/// 32 taps, beta 5: within +-0.05 dB of the ideal interpolator up to 0.45 fs. (16 taps at
/// beta 8 reads up to 0.6 dB low at 0.40 fs and 4 dB at 0.45 fs, more than the guard, so
/// HF-rich input such as pink noise overshot the ceiling by 0.2 dB.)
const TP_TAPS_PER_PHASE: usize = 32;
const TP_KAISER_BETA: f64 = 5.0;
const ABSOLUTE_CEILING_DBFS: f64 = -1.0;

/// Ratio-infinity soft knee (all in dB): unchanged below `c - w/2`, equal to `c` above
/// `c + w/2`, quadratic in between. Never exceeds `c`.
#[inline]
pub fn soft_knee_db(x: f64, c: f64, w: f64) -> f64 {
    if w <= 0.0 || x >= c + w / 2.0 {
        x.min(c)
    } else if x <= c - w / 2.0 {
        x
    } else {
        let d = x - c + w / 2.0;
        x - d * d / (2.0 * w)
    }
}

#[inline]
fn finite_or_zero(x: f32) -> f32 {
    if x.is_finite() { x } else { 0.0 }
}

/// Hard clamp to +-c that maps NaN to 0 (f32::clamp passes NaN through).
#[inline]
fn safe_clamp(y: f32, c: f32) -> f32 {
    if y.is_nan() { 0.0 } else { y.clamp(-c, c) }
}

const PHASES: usize = OVERSAMPLE - 1;
/// Delay of the true-peak detector: it needs this many samples after `n`.
const TP_DELAY: usize = TP_TAPS_PER_PHASE / 2;

/// Gain (linear, <= 1) that maps a peak `x` (linear, >= 0) onto the soft-knee curve
/// below ceiling `c`; `k0` is the knee start `c * 10^(-KNEE_DB/40)`.
#[inline]
fn need_gain(x: f32, c: f32, k0: f32) -> f32 {
    if x <= k0 {
        return 1.0;
    }
    let (xd, cd) = (20.0 * (x as f64).log10(), 20.0 * (c as f64).log10());
    let y = soft_knee_db(xd, cd, KNEE_DB);
    (10f64.powf((y - xd) / 20.0) as f32).min(1.0)
}

/// Zeroth-order modified Bessel function of the first kind (series).
fn bessel_i0(v: f64) -> f64 {
    let (mut sum, mut term) = (1.0, 1.0);
    for k in 1..60 {
        term *= (v / (2.0 * k as f64)).powi(2);
        sum += term;
    }
    sum
}

/// Interpolator taps for the phases 1/4, 2/4, 3/4. Tap `i` weights `x[n - TP_DELAY + 1 + i]`.
fn tp_taps() -> [[f32; TP_TAPS_PER_PHASE]; PHASES] {
    let half = TP_DELAY as f64;
    let mut taps = [[0.0f32; TP_TAPS_PER_PHASE]; PHASES];
    for (ph, row) in taps.iter_mut().enumerate() {
        let d = (ph + 1) as f64 / OVERSAMPLE as f64;
        for (i, c) in row.iter_mut().enumerate() {
            let t = (i as f64 + 1.0 - half) - d;
            let x = std::f64::consts::PI * t;
            let sinc = if t == 0.0 { 1.0 } else { x.sin() / x };
            let r = t / half;
            *c = (sinc * bessel_i0(TP_KAISER_BETA * (1.0 - r * r).max(0.0).sqrt()) / bessel_i0(TP_KAISER_BETA)) as f32;
        }
    }
    taps
}

/// Per-channel state; every buffer is allocated in `new`.
struct Chan {
    /// Signal delay, length latency + 1.
    delay: Vec<f32>,
    /// Last `TP_TAPS_PER_PHASE` inputs, stored twice so the window is one contiguous slice.
    hist: Vec<f32>,
    /// Monotonic deque (sample index, need) for the sliding minimum, capacity `L + 2`.
    dq_idx: Vec<u64>,
    dq_val: Vec<f32>,
    dq_head: usize,
    dq_len: usize,
    /// Last `L` window minima `h` and their sum.
    h_ring: Vec<f32>,
    h_sum: f64,
    g: f32,
}

impl Chan {
    fn new(lookahead: usize, latency: usize) -> Self {
        Self {
            delay: vec![0.0; latency + 1],
            hist: vec![0.0; 2 * TP_TAPS_PER_PHASE],
            dq_idx: vec![0; lookahead + 2],
            dq_val: vec![1.0; lookahead + 2],
            dq_head: 0,
            dq_len: 0,
            h_ring: vec![1.0; lookahead],
            h_sum: lookahead as f64,
            g: 1.0,
        }
    }

    /// Forgets all gain state (the signal history is kept).
    fn reset_gain(&mut self) {
        self.dq_head = 0;
        self.dq_len = 0;
        self.h_ring.fill(1.0);
        self.h_sum = self.h_ring.len() as f64;
        self.g = 1.0;
    }

    fn reset(&mut self) {
        self.delay.fill(0.0);
        self.hist.fill(0.0);
        self.reset_gain();
    }

    /// Stores `x` at the shared positions; returns the delayed sample `latency` frames ago.
    #[inline]
    fn push_signal(&mut self, x: f32, pos: usize, hpos: usize) -> f32 {
        self.delay[pos] = x;
        self.hist[hpos] = x;
        self.hist[hpos + TP_TAPS_PER_PHASE] = x;
        let o = if pos + 1 == self.delay.len() { 0 } else { pos + 1 };
        self.delay[o]
    }

    /// Updates the gain for sample index `t` (after `push_signal`) and returns it.
    #[inline]
    fn update_gain(&mut self, t: u64, hpos: usize, rpos: usize, s: &Shared) -> f32 {
        // Window x[t - TAPS + 1 ..= t]; the detected sample n = t - TP_DELAY is w[TP_DELAY - 1].
        let w = &self.hist[hpos + 1..hpos + 1 + TP_TAPS_PER_PHASE];
        let mut peak = w[TP_DELAY - 1].abs();
        for taps in &s.taps {
            let y: f32 = taps.iter().zip(w).map(|(a, b)| a * b).sum();
            peak = peak.max(y.abs());
        }
        let need = need_gain(peak, s.c_guard, s.k0);
        // Sliding minimum over the last L + 1 needs.
        let cap = self.dq_idx.len();
        while self.dq_len > 0 {
            let back = (self.dq_head + self.dq_len - 1) % cap;
            if self.dq_val[back] >= need {
                self.dq_len -= 1;
            } else {
                break;
            }
        }
        let slot = (self.dq_head + self.dq_len) % cap;
        self.dq_idx[slot] = t;
        self.dq_val[slot] = need;
        self.dq_len += 1;
        let span = self.h_ring.len() as u64 + 1;
        while self.dq_idx[self.dq_head] + span <= t {
            self.dq_head = (self.dq_head + 1) % cap;
            self.dq_len -= 1;
        }
        let h = self.dq_val[self.dq_head];
        // Moving average of h over the last L values; recomputed exactly once per lap so
        // the running sum cannot drift.
        self.h_sum += h as f64 - self.h_ring[rpos] as f64;
        self.h_ring[rpos] = h;
        if rpos + 1 == self.h_ring.len() {
            self.h_sum = self.h_ring.iter().map(|&v| v as f64).sum();
        }
        let gb = ((self.h_sum / self.h_ring.len() as f64) as f32).min(1.0);
        self.g = if gb < self.g { gb } else { self.g + (gb - self.g) * s.a_release };
        self.g
    }
}

/// Per-call constants shared by both channels.
struct Shared {
    taps: [[f32; TP_TAPS_PER_PHASE]; PHASES],
    c_guard: f32,
    k0: f32,
    a_release: f32,
}

pub struct Limiter {
    latency: usize,
    ch: [Chan; 2],
    /// Delay write position (0..=latency).
    pos: usize,
    /// Newest slot in `Chan::hist` (0..TP_TAPS_PER_PHASE).
    hpos: usize,
    /// Slot in `Chan::h_ring` (0..L).
    rpos: usize,
    /// Sample counter for the deque.
    t: u64,
    shared: Shared,
    /// Smallest gain applied per channel during the last `process` call.
    min_g: [f32; 2],
}

impl Limiter {
    /// Allocates. `max_frames` is the largest block `process` will see (any size works).
    pub fn new(fs: f64, _max_frames: usize) -> Self {
        let lookahead = ((LOOKAHEAD_S * fs).round() as usize).max(1);
        let latency = lookahead + TP_DELAY;
        Self {
            latency,
            ch: [Chan::new(lookahead, latency), Chan::new(lookahead, latency)],
            pos: 0,
            hpos: 0,
            rpos: 0,
            t: 0,
            shared: Shared {
                taps: tp_taps(),
                c_guard: 1.0,
                k0: 1.0,
                a_release: (1.0 - (-1.0 / (RELEASE_S * fs)).exp()) as f32,
            },
            min_g: [1.0; 2],
        }
    }

    /// Deepest gain reduction (dB, >= 0) per channel during the last `process`/`process_bypass`.
    pub fn last_reduction_db(&self) -> [f32; 2] {
        self.min_g.map(|g| -20.0 * g.max(1e-6).log10())
    }

    pub fn latency_frames(&self) -> usize {
        self.latency
    }

    pub fn reset(&mut self) {
        self.ch.iter_mut().for_each(Chan::reset);
        self.pos = 0;
        self.hpos = 0;
        self.rpos = 0;
        self.t = 0;
        self.min_g = [1.0; 2];
    }

    #[inline]
    fn advance(&mut self) {
        self.pos = if self.pos == self.latency { 0 } else { self.pos + 1 };
        self.hpos = if self.hpos + 1 == TP_TAPS_PER_PHASE { 0 } else { self.hpos + 1 };
        self.rpos = if self.rpos + 1 == self.ch[0].h_ring.len() { 0 } else { self.rpos + 1 };
        self.t += 1;
    }

    /// Bypass that keeps the same latency: delays the input and only hard-clamps at
    /// 0 dBFS (NaN-safe). Gain state is reset so re-enabling starts clean.
    pub fn process_bypass(&mut self, l: &mut [f32], r: &mut [f32]) {
        self.ch.iter_mut().for_each(Chan::reset_gain);
        self.min_g = [1.0; 2];
        for (xl, xr) in l.iter_mut().zip(r.iter_mut()) {
            let (pos, hpos) = (self.pos, self.hpos);
            *xl = safe_clamp(self.ch[0].push_signal(finite_or_zero(*xl), pos, hpos), 1.0);
            *xr = safe_clamp(self.ch[1].push_signal(finite_or_zero(*xr), pos, hpos), 1.0);
            self.advance();
        }
    }

    /// Realtime-safe: no allocation, no locks.
    pub fn process(&mut self, l: &mut [f32], r: &mut [f32], ceiling_dbfs: f64) {
        debug_assert_eq!(l.len(), r.len());
        let cd = ceiling_dbfs.min(ABSOLUTE_CEILING_DBFS);
        let c = 10f64.powf(cd / 20.0) as f32;
        self.shared.c_guard = 10f64.powf((cd - TP_GUARD_DB) / 20.0) as f32;
        self.shared.k0 = self.shared.c_guard * 10f32.powf(-(KNEE_DB as f32) / 40.0);
        let mut min_g = [1.0f32; 2];
        for (xl, xr) in l.iter_mut().zip(r.iter_mut()) {
            let (pos, hpos, rpos, t) = (self.pos, self.hpos, self.rpos, self.t);
            for (k, x) in [xl, xr].into_iter().enumerate() {
                // Non-finite input is replaced by silence so it cannot poison state.
                let ch = &mut self.ch[k];
                let d = ch.push_signal(finite_or_zero(*x), pos, hpos);
                let g = ch.update_gain(t, hpos, rpos, &self.shared);
                min_g[k] = min_g[k].min(g);
                *x = safe_clamp(d * g, c);
            }
            self.advance();
        }
        self.min_g = min_g;
    }
}
