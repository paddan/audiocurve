use audiocurve_dsp::*;

#[test]
fn ffi_roundtrip() {
    unsafe {
        let k = ac_kernel_new(48000.0, 512);
        assert!(!k.is_null());
        let mut p = Params::unity();
        p.output_gain_db = -6.0;
        ac_kernel_set_params(k, &p);
        let mut l = vec![0.1f32; 512];
        let mut r = vec![0.1f32; 512];
        ac_kernel_process(k, l.as_mut_ptr(), r.as_mut_ptr(), 512);
        let mut m = Meters::default();
        ac_kernel_read_meters(k, &mut m);
        assert!(m.in_rms_db[0] > -120.0);
        ac_kernel_free(k);
    }
}

#[test]
fn ffi_rejects_bad_args() {
    assert!(ac_kernel_new(0.0, 512).is_null());
    assert!(ac_kernel_new(48000.0, 0).is_null());
    unsafe {
        ac_kernel_free(std::ptr::null_mut());
        ac_kernel_set_params(std::ptr::null_mut(), std::ptr::null());
        ac_kernel_process(std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut(), 4);
        ac_kernel_reset(std::ptr::null_mut());
        ac_kernel_read_meters(std::ptr::null(), std::ptr::null_mut());
        assert_eq!(ac_kernel_latency_frames(std::ptr::null()), 0);
    }
}

#[test]
fn ffi_latency() {
    unsafe {
        let k = ac_kernel_new(48000.0, 512);
        // 2 ms limiter lookahead + 3 ms band-budget lookahead at 48 kHz.
        assert_eq!(ac_kernel_latency_frames(k), 240);
        ac_kernel_free(k);
    }
}
