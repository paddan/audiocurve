//! The hearing-test tone (Swift `ToneSynth`): three 200 ms pulses separated by 200 ms gaps,
//! phase-continuous, with raised-cosine ramps and a final hard level clamp.
//! Realtime-safe: `render` neither allocates nor locks.

use std::f64::consts::PI;

use crate::hearing_test::TestCalibration;
use crate::profile::Ear;

#[derive(Clone, Debug)]
pub struct ToneSynth {
    sample_rate: f64,
    ear: Ear,
    /// Linear peak; 0 = silent catch tone.
    amplitude: f64,
    cap_linear: f64,
    phase: f64,
    phase_step: f64,
    /// Samples rendered since start.
    position: usize,
    active: bool,
}

impl ToneSynth {
    pub const PULSE_ON: f64 = 0.2;
    pub const PULSE_OFF: f64 = 0.2;
    pub const PULSES: usize = 3;
    pub const RAMP: f64 = 0.02;
    pub const DURATION: f64 = 1.0;

    pub fn new(sample_rate: f64) -> Self {
        Self {
            sample_rate,
            ear: Ear::Left,
            amplitude: 0.0,
            cap_linear: 0.0,
            phase: 0.0,
            phase_step: 0.0,
            position: 0,
            active: false,
        }
    }

    fn total(&self) -> usize {
        (Self::DURATION * self.sample_rate) as usize
    }

    pub fn is_finished(&self) -> bool {
        !self.active || self.position >= self.total()
    }

    pub fn start(&mut self, ear: Ear, frequency_hz: f64, level_dbfs: Option<f64>) {
        self.ear = ear;
        self.cap_linear = 10f64.powf(TestCalibration::cap_dbfs(frequency_hz) / 20.0);
        // Never above the cap (a louder request plays a sine at the cap, not a clipped one);
        // a non-finite level is silent.
        self.amplitude = match level_dbfs {
            Some(l) if l.is_finite() => 10f64.powf(l / 20.0).min(self.cap_linear),
            _ => 0.0,
        };
        self.phase = 0.0;
        self.phase_step = 2.0 * PI * frequency_hz / self.sample_rate;
        self.position = 0;
        self.active = true;
    }

    /// Fills both slices (same length); the ear not tested gets silence.
    pub fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        let total = self.total();
        let period = Self::PULSE_ON + Self::PULSE_OFF;
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            let mut out = 0.0;
            if self.active && self.position < total && self.amplitude > 0.0 {
                let t = self.position as f64 / self.sample_rate;
                let k = (t / period) as usize;
                let local = t - k as f64 * period;
                if k < Self::PULSES && local < Self::PULSE_ON {
                    let env = if local < Self::RAMP {
                        0.5 - 0.5 * (PI * local / Self::RAMP).cos()
                    } else if local > Self::PULSE_ON - Self::RAMP {
                        0.5 - 0.5 * (PI * (Self::PULSE_ON - local) / Self::RAMP).cos()
                    } else {
                        1.0
                    };
                    out = self.amplitude * env * self.phase.sin();
                }
                self.phase += self.phase_step;
                if self.phase >= 2.0 * PI {
                    self.phase -= 2.0 * PI;
                }
            }
            if self.active {
                self.position += 1;
            }
            // Hard clamp, applied after all gain.
            let clamped = out.clamp(-self.cap_linear, self.cap_linear) as f32;
            *l = if self.ear == Ear::Left { clamped } else { 0.0 };
            *r = if self.ear == Ear::Right { clamped } else { 0.0 };
        }
    }
}
