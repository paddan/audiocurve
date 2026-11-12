//! Speech/music classifier for the Auto mode (audio thread, no allocation after `new`).
//!
//! Speech has deep dips between syllables, words and phrases; music (even with drums) rarely
//! drops far below its recent loud level. Every 100 ms the classifier looks at the last 4 s of
//! 10 ms frame levels and measures the share of frames more than `DIP_DB` below the window's
//! 90th-percentile level. That share is smoothed (tau 3 s), decided with hysteresis, and the
//! result glides (tau 1.5 s) so the compressor time constants never jump.
//! Silence (window below `SILENCE_DBFS`) freezes the decision, and after a long digital
//! silence (track gap, pause) the window starts over so the gap is not counted as dips.

const FRAME_S: f64 = 0.010;
const WINDOW_FRAMES: usize = 400; // 4 s
const UPDATE_FRAMES: usize = 10; // 100 ms
const DIP_DB: f32 = 25.0;
const SILENCE_DBFS: f32 = -55.0;
/// Dip share mapped linearly from music (<= LOW) to speech (>= HIGH).
const DIP_SHARE_LOW: f32 = 0.05;
const DIP_SHARE_HIGH: f32 = 0.20;
const SCORE_TAU_S: f64 = 3.0;
const BLEND_TAU_S: f64 = 1.5;
const HYST_ON: f32 = 0.7;
const HYST_OFF: f32 = 0.35;
/// Frames below this are digital silence; this many in a row restart the window.
const GAP_DBFS: f32 = -80.0;
const GAP_FRAMES: usize = 100; // 1 s

pub struct Classifier {
    frame_len: usize,
    acc: f64,
    n: usize,
    ring: Vec<f32>,
    scratch: Vec<f32>,
    pos: usize,
    filled: usize,
    since_update: usize,
    quiet_run: usize,
    score: f32,
    target: f32,
    speech: f32,
    a_score: f32,
    a_blend: f32,
}

impl Classifier {
    pub fn new(fs: f64) -> Self {
        let frame_len = ((FRAME_S * fs).round() as usize).max(1);
        Self {
            frame_len,
            acc: 0.0,
            n: 0,
            ring: vec![-120.0; WINDOW_FRAMES],
            scratch: vec![0.0; WINDOW_FRAMES],
            pos: 0,
            filled: 0,
            since_update: 0,
            quiet_run: 0,
            score: 0.5,
            target: 0.5,
            speech: 0.5,
            a_score: (1.0 - (-(UPDATE_FRAMES as f64 * FRAME_S) / SCORE_TAU_S).exp()) as f32,
            a_blend: (1.0 - (-FRAME_S / BLEND_TAU_S).exp()) as f32,
        }
    }

    pub fn reset(&mut self) {
        self.acc = 0.0;
        self.n = 0;
        self.ring.fill(-120.0);
        self.pos = 0;
        self.filled = 0;
        self.since_update = 0;
        self.quiet_run = 0;
        self.score = 0.5;
        self.target = 0.5;
        self.speech = 0.5;
    }

    /// Speech-likeness, 0 = music … 1 = speech (smoothed, safe to drive time constants).
    pub fn speech(&self) -> f32 {
        self.speech
    }

    /// Smoothed raw score before hysteresis (diagnostics).
    pub fn score(&self) -> f32 {
        self.score
    }

    pub fn process(&mut self, l: &[f32], r: &[f32]) {
        for (a, b) in l.iter().zip(r) {
            let m = 0.5 * (*a as f64 + *b as f64);
            self.acc += m * m;
            self.n += 1;
            if self.n == self.frame_len {
                let db = (10.0 * (2.0 * self.acc / self.n as f64 + 1e-20).log10()) as f32;
                self.acc = 0.0;
                self.n = 0;
                self.push_frame(db.max(-120.0));
            }
        }
    }

    fn push_frame(&mut self, db: f32) {
        if db < GAP_DBFS {
            self.quiet_run += 1;
            if self.quiet_run >= GAP_FRAMES {
                // Long gap: forget the window (keep the decision) and skip the silence.
                if self.quiet_run == GAP_FRAMES {
                    self.pos = 0;
                    self.filled = 0;
                    self.since_update = 0;
                }
                self.speech += (self.target - self.speech) * self.a_blend;
                return;
            }
        } else {
            self.quiet_run = 0;
        }
        self.ring[self.pos] = db;
        self.pos = (self.pos + 1) % WINDOW_FRAMES;
        self.filled = (self.filled + 1).min(WINDOW_FRAMES);
        self.since_update += 1;
        if self.since_update >= UPDATE_FRAMES && self.filled >= WINDOW_FRAMES {
            self.since_update = 0;
            self.update_score();
        }
        self.speech += (self.target - self.speech) * self.a_blend;
    }

    fn update_score(&mut self) {
        let n = self.filled;
        let s = &mut self.scratch[..n];
        s.copy_from_slice(&self.ring[..n]);
        let k = ((n - 1) as f32 * 0.9) as usize;
        let (_, p90, _) = s.select_nth_unstable_by(k, |a, b| a.total_cmp(b));
        let p90 = *p90;
        if p90 < SILENCE_DBFS {
            return;
        }
        let dips = self.ring[..n].iter().filter(|&&v| v < p90 - DIP_DB).count() as f32 / n as f32;
        let raw = ((dips - DIP_SHARE_LOW) / (DIP_SHARE_HIGH - DIP_SHARE_LOW)).clamp(0.0, 1.0);
        self.score += (raw - self.score) * self.a_score;
        if self.score > HYST_ON {
            self.target = 1.0;
        } else if self.score < HYST_OFF {
            self.target = 0.0;
        }
    }
}
