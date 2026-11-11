//! Lookahead peak limiter: the hearing-safety backstop (MPO).
//!
//! Output = delayed input * g, followed by a hard clamp to +-c, so the ceiling
//! is a guarantee. `g` follows the minimum of the needed gain over the
//! lookahead window (attack tau 0.5 ms, release tau 50 ms).

const LOOKAHEAD_S: f64 = 0.002;
const ATTACK_S: f64 = 0.0005;
const RELEASE_S: f64 = 0.050;
const ABSOLUTE_CEILING_DBFS: f64 = -1.0;

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
    need: Vec<f32>,
    pos: usize,
    g: f32,
    a_attack: f32,
    a_release: f32,
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
            need: vec![1.0; latency + 1],
            pos: 0,
            g: 1.0,
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
        self.need.fill(1.0);
        self.pos = 0;
        self.g = 1.0;
    }

    /// Realtime-safe: no allocation, no locks.
    pub fn process(&mut self, l: &mut [f32], r: &mut [f32], ceiling_dbfs: f64) {
        debug_assert_eq!(l.len(), r.len());
        let c = 10f64.powf(ceiling_dbfs.min(ABSOLUTE_CEILING_DBFS) / 20.0) as f32;
        let len = self.dl.len();
        for (xl, xr) in l.iter_mut().zip(r.iter_mut()) {
            let p = self.pos;
            // Non-finite input is replaced by silence so it cannot poison state.
            let (il, ir) = (finite_or_zero(*xl), finite_or_zero(*xr));
            self.dl[p] = il;
            self.dr[p] = ir;
            let peak = il.abs().max(ir.abs());
            self.need[p] = if peak > c { c / peak } else { 1.0 };
            let target = self.need.iter().fold(1.0f32, |m, &v| m.min(v));
            let a = if target < self.g { self.a_attack } else { self.a_release };
            self.g = target + a * (self.g - target);
            // Oldest entry (input from `latency` samples ago).
            let o = if p + 1 == len { 0 } else { p + 1 };
            *xl = safe_clamp(self.dl[o] * self.g, c);
            *xr = safe_clamp(self.dr[o] * self.g, c);
            self.pos = o;
        }
    }
}
