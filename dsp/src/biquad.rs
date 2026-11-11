//! Transposed direct form II biquad with RBJ cookbook coefficients.
//! Coefficients are f64; state is f32-stored but accumulated in f64 per step.

#[derive(Clone, Copy, Debug)]
pub struct Biquad {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
    z1: f32,
    z2: f32,
}

impl Biquad {
    fn from_raw(b0: f64, b1: f64, b2: f64, a0: f64, a1: f64, a2: f64) -> Self {
        Self { b0: b0 / a0, b1: b1 / a0, b2: b2 / a0, a1: a1 / a0, a2: a2 / a0, z1: 0.0, z2: 0.0 }
    }

    fn trig(fc: f64, fs: f64, q: f64) -> (f64, f64, f64) {
        let w0 = 2.0 * std::f64::consts::PI * fc / fs;
        (w0.cos(), w0.sin(), w0.sin() / (2.0 * q))
    }

    /// Pass-through section (`y = x`).
    pub fn identity() -> Self {
        Self { b0: 1.0, b1: 0.0, b2: 0.0, a1: 0.0, a2: 0.0, z1: 0.0, z2: 0.0 }
    }

    /// Section with zero output (`y = 0`).
    pub fn zero() -> Self {
        Self { b0: 0.0, b1: 0.0, b2: 0.0, a1: 0.0, a2: 0.0, z1: 0.0, z2: 0.0 }
    }

    pub fn lowpass(fc: f64, fs: f64, q: f64) -> Self {
        let (c, _s, alpha) = Self::trig(fc, fs, q);
        Self::from_raw((1.0 - c) / 2.0, 1.0 - c, (1.0 - c) / 2.0, 1.0 + alpha, -2.0 * c, 1.0 - alpha)
    }

    pub fn highpass(fc: f64, fs: f64, q: f64) -> Self {
        let (c, _s, alpha) = Self::trig(fc, fs, q);
        Self::from_raw((1.0 + c) / 2.0, -(1.0 + c), (1.0 + c) / 2.0, 1.0 + alpha, -2.0 * c, 1.0 - alpha)
    }

    pub fn allpass(fc: f64, fs: f64, q: f64) -> Self {
        let (c, _s, alpha) = Self::trig(fc, fs, q);
        Self::from_raw(1.0 - alpha, -2.0 * c, 1.0 + alpha, 1.0 + alpha, -2.0 * c, 1.0 - alpha)
    }

    #[inline]
    pub fn process(&mut self, x: f32) -> f32 {
        let xd = x as f64;
        let y = self.b0 * xd + self.z1 as f64;
        self.z1 = (self.b1 * xd - self.a1 * y + self.z2 as f64) as f32;
        self.z2 = (self.b2 * xd - self.a2 * y) as f32;
        y as f32
    }

    pub fn reset(&mut self) {
        self.z1 = 0.0;
        self.z2 = 0.0;
    }
}
