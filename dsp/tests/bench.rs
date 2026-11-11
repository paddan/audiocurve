mod support;
use audiocurve_dsp::*;
use std::time::Instant;
use support::*;

#[test]
#[ignore]
fn under_budget() {
    let fs = 48000.0;
    for amp in [0.1f32, 1e-38] {
        let h = Handle::new(fs, 512).unwrap();
        let mut p = Params::unity();
        p.left = [BandFitting { gain_soft_db: 60.0, gain_loud_db: 25.0, compression_ratio: 4.5 }; 12];
        p.right = p.left;
        h.set_params(&p);
        let src = noise(amp, 512, 3);
        let (mut l, mut r) = (src.clone(), src.clone());
        let t = Instant::now();
        for _ in 0..2000 {
            l.copy_from_slice(&src);
            r.copy_from_slice(&src);
            unsafe { h.process(&mut l, &mut r) };
        }
        let per = t.elapsed().as_secs_f64() / 2000.0;
        let budget = 0.10 * 512.0 / fs;
        println!("amp {amp:e}: {:.1} us/block (budget {:.1} us)", per * 1e6, budget * 1e6);
        assert!(per < budget);
    }
}
