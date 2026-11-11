mod support;
use audiocurve_dsp::compressor::BandCompressor;
use audiocurve_dsp::params::gain_db;
use audiocurve_dsp::BandFitting;
use support::*;

const FS: f64 = 48_000.0;

fn fitting() -> BandFitting {
    BandFitting { gain_soft_db: 40., gain_loud_db: 20., compression_ratio: 1.8 }
}

#[test]
fn steady_state_gain() {
    let b = fitting();
    for level in [-60., -40., -15.] {
        let x = sine(1000.0, level, 1.0, FS);
        let mut y = x.clone();
        let mut c = BandCompressor::new(FS);
        c.process(&mut y, &b, 5.0, 50.0);
        let out = level_dbfs(&y, 0.5, FS);
        let expected = level + gain_db(&b, level);
        let err = (out - expected).abs();
        assert!(
            err <= 1.0,
            "level {level} dBFS: output {out:.3} dBFS, expected {expected:.3} dBFS, error {err:.3} dB"
        );
    }
}

/// Time in ms until the 10 ms-window gain (out dB − in dB) first lies within 1 dB of
/// `gain_db(b, -50)`, measured from the step down to -50 dBFS.
fn recovery_ms(release_ms: f64) -> Option<f64> {
    let b = fitting();
    let target = gain_db(&b, -50.0);
    let step = (0.5 * FS) as usize;
    let mut x = sine(1000.0, -15.0, 3.5, FS);
    let k = 10f64.powf(-35.0 / 20.0) as f32;
    for v in &mut x[step..] {
        *v *= k;
    }
    let mut y = x.clone();
    let mut c = BandCompressor::new(FS);
    c.process(&mut y, &b, 5.0, release_ms);

    let win = (0.010 * FS) as usize;
    let mut start = step;
    let mut idx = 0usize;
    while start + win <= x.len() {
        let gain = level_dbfs(&y[start..start + win], 0.0, FS)
            - level_dbfs(&x[start..start + win], 0.0, FS);
        if (gain - target).abs() <= 1.0 {
            return Some(idx as f64 * 10.0);
        }
        start += win;
        idx += 1;
    }
    None
}

#[test]
fn music_release_is_slower() {
    let t50 = recovery_ms(50.0).expect("release 50 ms never recovered to gain_db(-50)");
    let t200 = recovery_ms(200.0).expect("release 200 ms never recovered to gain_db(-50)");
    assert!(
        t200 > 2.5 * t50,
        "recovery times: release 50 ms = {t50} ms, release 200 ms = {t200} ms (need > 2.5x)"
    );
}

#[test]
fn silence_stays_zero() {
    let b = BandFitting { gain_soft_db: 60., gain_loud_db: 25., compression_ratio: 4.5 };
    let mut x = vec![0.0f32; FS as usize];
    let mut c = BandCompressor::new(FS);
    c.process(&mut x, &b, 5.0, 50.0);
    let peak = x.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    assert_eq!(peak, 0.0);
}
