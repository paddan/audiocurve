//! Lookahead peak limiter: the hearing-safety backstop (MPO).
//!
//! Output = delayed input * g (per channel, unlinked), followed by a hard clamp to +-c, so the ceiling
//! is a guarantee. `g` follows the minimum of the needed gain over the
//! lookahead window (attack tau 0.5 ms, release tau 150 ms). The needed gain uses a
//! soft knee: peaks from KNEE_DB/2 below the ceiling are eased down gradually.

const LOOKAHEAD_S: f64 = 0.002;
const ATTACK_S: f64 = 0.0005;
const RELEASE_S: f64 = 0.150;
const KNEE_DB: f64 = 3.0;
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

pub struct Limiter {
    latency: usize,
    /// Ring length = latency + 1.
    dl: Vec<f32>,
    dr: Vec<f32>,
    need_l: Vec<f32>,
    need_r: Vec<f32>,
    pos: usize,
    g_l: f32,
    g_r: f32,
    a_attack: f32,
    a_release: f32,
}

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

impl Limiter {
    /// Allocates. `max_frames` is the largest block `process` will see (any size works).
    pub fn new(fs: f64, _max_frames: usize) -> Self {
        let latency = (LOOKAHEAD_S * fs).round() as usize;
        let coef = |tau: f64| (-1.0 / (tau * fs)).exp() as f32;
        Self {
            latency,
            dl: vec![0.0; latency + 1],
            dr: vec![0.0; latency + 1],
            need_l: vec![1.0; latency + 1],
            need_r: vec![1.0; latency + 1],
            pos: 0,
            g_l: 1.0,
            g_r: 1.0,
            a_attack: coef(ATTACK_S),
            a_release: coef(RELEASE_S),
        }
    }

    pub fn latency_frames(&self) -> usize {
        self.latency
    }

    pub fn reset(&mut self) {
        self.dl.fill(0.0);
        self.dr.fill(0.0);
        self.need_l.fill(1.0);
        self.need_r.fill(1.0);
        self.pos = 0;
        self.g_l = 1.0;
        self.g_r = 1.0;
    }

    /// Bypass that keeps the same latency: delays the input and only hard-clamps at
    /// 0 dBFS (NaN-safe). Gain state is reset so re-enabling starts clean.
    pub fn process_bypass(&mut self, l: &mut [f32], r: &mut [f32]) {
        let len = self.dl.len();
        self.need_l.fill(1.0);
        self.need_r.fill(1.0);
        self.g_l = 1.0;
        self.g_r = 1.0;
        for (xl, xr) in l.iter_mut().zip(r.iter_mut()) {
            let p = self.pos;
            self.dl[p] = finite_or_zero(*xl);
            self.dr[p] = finite_or_zero(*xr);
            let o = if p + 1 == len { 0 } else { p + 1 };
            *xl = safe_clamp(self.dl[o], 1.0);
            *xr = safe_clamp(self.dr[o], 1.0);
            self.pos = o;
        }
    }

    /// Realtime-safe: no allocation, no locks.
    pub fn process(&mut self, l: &mut [f32], r: &mut [f32], ceiling_dbfs: f64) {
        debug_assert_eq!(l.len(), r.len());
        let c = 10f64.powf(ceiling_dbfs.min(ABSOLUTE_CEILING_DBFS) / 20.0) as f32;
        let k0 = c * 10f32.powf(-(KNEE_DB as f32) / 40.0);
        let len = self.dl.len();
        for (xl, xr) in l.iter_mut().zip(r.iter_mut()) {
            let p = self.pos;
            // Non-finite input is replaced by silence so it cannot poison state.
            let (il, ir) = (finite_or_zero(*xl), finite_or_zero(*xr));
            self.dl[p] = il;
            self.dr[p] = ir;
            self.need_l[p] = need_gain(il.abs(), c, k0);
            self.need_r[p] = need_gain(ir.abs(), c, k0);
            let tl = self.need_l.iter().fold(1.0f32, |m, &v| m.min(v));
            let tr = self.need_r.iter().fold(1.0f32, |m, &v| m.min(v));
            let al = if tl < self.g_l { self.a_attack } else { self.a_release };
            let ar = if tr < self.g_r { self.a_attack } else { self.a_release };
            self.g_l = tl + al * (self.g_l - tl);
            self.g_r = tr + ar * (self.g_r - tr);
            // Oldest entry (input from `latency` samples ago).
            let o = if p + 1 == len { 0 } else { p + 1 };
            *xl = safe_clamp(self.dl[o] * self.g_l, c);
            *xr = safe_clamp(self.dr[o] * self.g_r, c);
            self.pos = o;
        }
    }
}
