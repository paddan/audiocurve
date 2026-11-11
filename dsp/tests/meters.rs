mod support;
use audiocurve_dsp::meters::{AtomicMeters, MeterState};
use support::*;

const FS: f64 = 48000.0;

#[test]
fn meters_sine() {
    let x = sine(1000.0, -20.0, 2.0, FS);
    let mut m = MeterState::new(FS);
    let (mut rms, mut peak) = ([0.0; 2], [0.0; 2]);
    for c in x.chunks(512) {
        (rms, peak) = m.update(c, c);
    }
    println!("rms {rms:?} peak {peak:?}");
    for ch in 0..2 {
        assert!((rms[ch] + 20.0).abs() < 0.5);
        assert!((peak[ch] + 20.0).abs() < 0.5);
    }
}

#[test]
fn meters_silence() {
    let x = vec![0.0f32; (2.0 * FS) as usize];
    let mut m = MeterState::new(FS);
    let (mut rms, mut peak) = ([0.0; 2], [0.0; 2]);
    for c in x.chunks(512) {
        (rms, peak) = m.update(c, c);
    }
    assert!(rms.iter().chain(peak.iter()).all(|&v| v <= -100.0));
}

#[test]
fn meters_peak_decays() {
    let mut m = MeterState::new(FS);
    let mut one = vec![0.0f32; 480];
    one[0] = 1.0;
    let z = vec![0.0f32; 480];
    let (_, p) = m.update(&one, &z);
    assert!(p[0].abs() < 1e-3 && p[1] <= -119.0);
    let mut p = p;
    for _ in 0..50 {
        (_, p) = m.update(&z, &z); // 0.5 s => -40 dB
    }
    assert!((p[0] + 40.0).abs() < 0.5, "{}", p[0]);
}

#[test]
fn atomic_meters_roundtrip() {
    let a = AtomicMeters::new();
    assert_eq!(a.load().in_rms_db, [-120.0; 2]);
    a.store_in([-1.0, -2.0], [-3.0, -4.0]);
    a.store_out([-5.0, -6.0], [-7.0, -8.0]);
    let m = a.load();
    assert_eq!(m.in_rms_db, [-1.0, -2.0]);
    assert_eq!(m.in_peak_db, [-3.0, -4.0]);
    assert_eq!(m.out_rms_db, [-5.0, -6.0]);
    assert_eq!(m.out_peak_db, [-7.0, -8.0]);
}
