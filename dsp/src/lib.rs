//! audiocurve-dsp: realtime hearing-compensation DSP behind a C ABI.
//! The FFI functions below are temporary stubs (replaced when the kernel is implemented).

pub mod bands;
pub mod biquad;
pub mod compressor;
pub mod filterbank;
pub mod params;

pub use bands::{BAND_COUNT, CENTERS, crossovers};
pub use biquad::Biquad;
pub use filterbank::{BandBuffers, Filterbank};
pub use params::{BandFitting, Meters, Params, gain_db};

/// Placeholder kernel object.
pub struct Kernel {
    _sample_rate: f64,
    _max_frames: u32,
}

#[unsafe(no_mangle)]
pub extern "C" fn ac_kernel_new(sample_rate: f64, max_frames: u32) -> *mut Kernel {
    if !(sample_rate.is_finite() && sample_rate > 0.0) || max_frames == 0 {
        return std::ptr::null_mut();
    }
    Box::into_raw(Box::new(Kernel { _sample_rate: sample_rate, _max_frames: max_frames }))
}

/// # Safety
/// `k` must be null or a pointer returned by `ac_kernel_new`, not yet freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ac_kernel_free(k: *mut Kernel) {
    if !k.is_null() {
        drop(unsafe { Box::from_raw(k) });
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn ac_kernel_set_params(_k: *mut Kernel, _p: *const Params) {}

#[unsafe(no_mangle)]
pub extern "C" fn ac_kernel_process(_k: *mut Kernel, _left: *mut f32, _right: *mut f32, _frames: u32) {}

#[unsafe(no_mangle)]
pub extern "C" fn ac_kernel_latency_frames(_k: *const Kernel) -> u32 {
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn ac_kernel_reset(_k: *mut Kernel) {}

/// # Safety
/// `out` must be null or point to a writable `Meters`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ac_kernel_read_meters(_k: *const Kernel, out: *mut Meters) {
    if !out.is_null() {
        unsafe {
            *out = Meters {
                in_rms_db: [-120.0; 2],
                in_peak_db: [-120.0; 2],
                out_rms_db: [-120.0; 2],
                out_peak_db: [-120.0; 2],
            };
        }
    }
}
