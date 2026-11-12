use audiocurve_dsp::classifier::Classifier;

const FS: f64 = 48000.0;

/// Deterministic pseudo-random in [0, 1).
struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> f64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// Speech-like: voiced syllables (150–300 ms) separated by 50–250 ms pauses at a -60 dBFS
/// noise floor, with a longer phrase pause every ~2 s.
fn speechlike(secs: f64) -> Vec<f32> {
    let n = (secs * FS) as usize;
    let mut x = vec![0f32; n];
    let mut rng = Lcg(7);
    let mut t = 0usize;
    let mut since_phrase = 0.0;
    while t < n {
        let syl = ((0.15 + 0.15 * rng.next()) * FS) as usize;
        let f0 = 110.0 + 60.0 * rng.next();
        for k in 0..syl.min(n - t) {
            let w = (std::f64::consts::PI * k as f64 / syl as f64).sin();
            let ph = 2.0 * std::f64::consts::PI * f0 * (t + k) as f64 / FS;
            let v: f64 = (1..8).map(|h| (h as f64 * ph).sin() / h as f64).sum();
            x[t + k] = (0.2 * w * v) as f32;
        }
        t += syl;
        since_phrase += syl as f64 / FS;
        let pause = if since_phrase > 2.0 { since_phrase = 0.0; 0.5 } else { 0.05 + 0.2 * rng.next() };
        t += (pause * FS) as usize;
    }
    for v in x.iter_mut() { *v += (0.001 * (rng.next() - 0.5)) as f32; }
    x
}

/// Music-like: a sustained chord with a drum-like 2 Hz accent (dips stay within ~8 dB).
fn musiclike(secs: f64) -> Vec<f32> {
    let n = (secs * FS) as usize;
    (0..n).map(|i| {
        let t = i as f64 / FS;
        let chord: f64 = [196.0, 247.0, 294.0, 392.0].iter().map(|f| (2.0 * std::f64::consts::PI * f * t).sin()).sum();
        let beat = (t * 2.0).fract();
        let env = 0.4 + 0.6 * (-beat * 8.0).exp();
        (0.08 * env * chord) as f32
    }).collect()
}

fn run(c: &mut Classifier, x: &[f32]) {
    for ch in x.chunks(512) { c.process(ch, ch); }
}

#[test]
fn speechlike_is_detected() {
    let mut c = Classifier::new(FS);
    run(&mut c, &speechlike(20.0));
    assert!(c.speech() > 0.9, "speech {}", c.speech());
}

#[test]
fn musiclike_is_music() {
    let mut c = Classifier::new(FS);
    run(&mut c, &musiclike(20.0));
    assert!(c.speech() < 0.1, "speech {}", c.speech());
}

#[test]
fn switches_back_and_forth_smoothly() {
    let mut c = Classifier::new(FS);
    run(&mut c, &musiclike(15.0));
    let mut prev = c.speech();
    let mut max_step = 0f32;
    for ch in speechlike(25.0).chunks(512) {
        c.process(ch, ch);
        max_step = max_step.max((c.speech() - prev).abs());
        prev = c.speech();
    }
    assert!(c.speech() > 0.9, "after speech {}", c.speech());
    assert!(max_step < 0.02, "step {max_step}"); // 1.5 s glide: ~1 % per 10 ms block
    run(&mut c, &musiclike(25.0));
    assert!(c.speech() < 0.1, "after music {}", c.speech());
}

#[test]
fn track_gap_and_silence_keep_the_decision() {
    let mut c = Classifier::new(FS);
    run(&mut c, &musiclike(15.0));
    run(&mut c, &vec![0f32; (5.0 * FS) as usize]);
    assert!(c.speech() < 0.1, "during gap {}", c.speech());
    // The next track's first seconds (gap edge) must not flip it to speech.
    let mut peak = 0f32;
    for ch in musiclike(10.0).chunks(512) { c.process(ch, ch); peak = peak.max(c.speech()); }
    assert!(peak < 0.1, "peak after gap {peak}");
}

#[test]
fn non_finite_input_is_harmless() {
    let mut c = Classifier::new(FS);
    let x = vec![f32::NAN; 48000];
    run(&mut c, &x);
    assert!(c.speech().is_finite());
}
