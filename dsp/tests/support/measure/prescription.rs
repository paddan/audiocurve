//! Fitting rules used by the measurements: a Rust mirror of the Swift `FittingRule`
//! (spec §4.2), with strength 1 and no trim.

use super::{Audiogram, Ear, Mode};
use audiocurve_dsp::{BandFitting, CENTERS};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Rx {
    Speech,
    Music,
}

fn hl_at(pts: &[(f64, f64)], f: f64) -> f64 {
    if f <= pts[0].0 {
        return pts[0].1;
    }
    if f >= pts[pts.len() - 1].0 {
        return pts[pts.len() - 1].1;
    }
    for i in 1..pts.len() {
        if f <= pts[i].0 {
            let (lo, hi) = (pts[i - 1], pts[i]);
            let t = (f.ln() - lo.0.ln()) / (hi.0.ln() - lo.0.ln());
            return lo.1 + t * (hi.1 - lo.1);
        }
    }
    unreachable!()
}

/// One prescription for one ear: `pts` are (Hz, dB HL), `tone` 0...1, `strength` 0...1.
pub fn rule(pts: &[(f64, f64)], rx: Rx, tone: f64, strength: f64) -> [BandFitting; 12] {
    let t = tone.clamp(0.0, 1.0);
    let s = strength.clamp(0.0, 1.0);
    let mut out = [BandFitting::UNITY; 12];
    for (i, &f) in CENTERS.iter().enumerate() {
        let hl = hl_at(pts, f);
        let hle = if hl <= 60.0 { hl } else { 60.0 + 0.5 * (hl - 60.0) };
        let (soft, loud) = match rx {
            Rx::Speech => {
                let corr = if f <= 250.0 { 10.0 } else if f == 500.0 { 5.0 } else { 0.0 };
                (0.6 * hle - corr, 0.25 * hle - corr)
            }
            Rx::Music => (0.45 * hle, if f <= 2000.0 { 0.3 * hle } else { 0.2 * hle }),
        };
        let boost = if f <= 250.0 { 5.0 } else if f == 500.0 { 3.0 } else if f == 750.0 || f == 1000.0 { 1.0 } else { 0.0 };
        // Added after the floor so the speech low-frequency correction cannot swallow it.
        let tone_soft = soft.max(0.0) + boost * t;
        let tone_loud = (loud - if f > 2000.0 { 6.0 * t } else { 0.0 }).max(0.0) + boost * t;
        let gs = tone_soft * s;
        let gl = tone_loud * s;
        let den = 60.0 - (gs - gl);
        let max_cr = if rx == Rx::Speech { 3.0 } else { 2.0 };
        let cr = if den <= 0.0 { max_cr } else { (60.0 / den).clamp(1.0, max_cr) };
        out[i] = BandFitting { gain_soft_db: gs, gain_loud_db: gl, compression_ratio: cr };
    }
    out
}

/// (speech, music) fittings of one ear.
pub fn pair(a: &Audiogram, ear: Ear, tone: f64) -> ([BandFitting; 12], [BandFitting; 12]) {
    let pts = match ear {
        Ear::Left => a.left,
        Ear::Right => a.right,
    };
    (rule(pts, Rx::Speech, tone, 1.0), rule(pts, Rx::Music, tone, 1.0))
}

/// The fitting a mode is measured with (Auto is measured at the Music set, weight 0).
pub fn fittings(a: &Audiogram, ear: Ear, mode: Mode, tone: f64) -> [BandFitting; 12] {
    let (speech, music) = pair(a, ear, tone);
    if mode == Mode::Speech { speech } else { music }
}
