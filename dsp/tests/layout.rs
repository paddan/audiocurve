use audiocurve_dsp::*;
#[test] fn params_is_608_bytes() { assert_eq!(std::mem::size_of::<Params>(), 608); }
#[test] fn meters_is_32_bytes() { assert_eq!(std::mem::size_of::<Meters>(), 32); }
#[test] fn centers_and_crossovers() {
    assert_eq!(CENTERS, [125.,250.,500.,750.,1000.,1500.,2000.,3000.,4000.,6000.,8000.,12000.]);
    let x = crossovers(); assert!((x[0]-176.777).abs() < 0.01 && (x[10]-9797.959).abs() < 0.01);
}
#[test] fn gain_curve() {
    let b = BandFitting { gain_soft_db: 60., gain_loud_db: 25., compression_ratio: 4.5 };
    assert_eq!(gain_db(&b, -90.), 45.); assert_eq!(gain_db(&b, -70.), 60.);
    assert_eq!(gain_db(&b, -60.), 60.); assert!((gain_db(&b, -15.) - 25.).abs() < 1e-9);
}
