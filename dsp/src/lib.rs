//! audiocurve-dsp: realtime hearing-compensation DSP behind a C ABI.

pub mod bands;
pub mod biquad;
pub mod compressor;
pub mod filterbank;
pub mod kernel;
pub mod limiter;
pub mod meters;
pub mod params;
pub mod spectrum;
pub mod triple;

pub use bands::{BAND_COUNT, CENTERS, crossovers};
pub use biquad::Biquad;
pub use filterbank::{BandBuffers, Filterbank};
pub use params::{BandFitting, Meters, Params, gain_db};
pub use kernel::Handle;
pub use spectrum::{SPECTRUM_FFT, SPECTRUM_POINTS, Spectrum};

/// Opaque C type `ac_kernel`.
pub type Kernel = Handle;

/// Returns a new kernel, or null for invalid arguments.
#[unsafe(no_mangle)]
pub extern "C" fn ac_kernel_new(sample_rate: f64, max_frames: u32) -> *mut Kernel {
    match Handle::new(sample_rate, max_frames as usize) {
        Some(b) => Box::into_raw(b),
        None => std::ptr::null_mut(),
    }
}

/// # Safety
/// `k` must be null or a pointer returned by `ac_kernel_new`, not yet freed, and no other
/// thread may be using it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ac_kernel_free(k: *mut Kernel) {
    if !k.is_null() {
        drop(unsafe { Box::from_raw(k) });
    }
}

/// Copies `*p`.
///
/// # Safety
/// `k` must be null or valid, `p` null or point to a readable `Params`. Must be called from
/// a single thread at a time.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ac_kernel_set_params(k: *mut Kernel, p: *const Params) {
    if let (Some(k), Some(p)) = (unsafe { k.as_ref() }, unsafe { p.as_ref() }) {
        k.set_params(p);
    }
}

/// # Safety
/// `k` must be null or valid; `left`/`right` null or valid for `frames` floats each. Must be
/// called from a single (audio) thread at a time.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ac_kernel_process(k: *mut Kernel, left: *mut f32, right: *mut f32, frames: u32) {
    if k.is_null() || left.is_null() || right.is_null() || frames == 0 {
        return;
    }
    unsafe {
        let l = std::slice::from_raw_parts_mut(left, frames as usize);
        let r = std::slice::from_raw_parts_mut(right, frames as usize);
        (*k).process(l, r);
    }
}

/// # Safety
/// `k` must be null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ac_kernel_latency_frames(k: *const Kernel) -> u32 {
    unsafe { k.as_ref() }.map_or(0, |k| k.latency_frames())
}

/// # Safety
/// `k` must be null or valid; same threading contract as `ac_kernel_process`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ac_kernel_reset(k: *mut Kernel) {
    if let Some(k) = unsafe { k.as_ref() } {
        unsafe { k.reset() };
    }
}

/// # Safety
/// `k` must be null or valid; `out` null or point to a writable `Meters`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ac_kernel_read_meters(k: *const Kernel, out: *mut Meters) {
    if let (Some(k), false) = (unsafe { k.as_ref() }, out.is_null()) {
        unsafe { *out = k.meters() };
    }
}

/// Fills `*out` with the current input/output spectrum. UI thread; not realtime.
///
/// # Safety
/// `k` must be null or valid; `out` null or point to a writable `Spectrum`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ac_kernel_read_spectrum(k: *const Kernel, out: *mut Spectrum) {
    if let (Some(k), false) = (unsafe { k.as_ref() }, out.is_null()) {
        k.read_spectrum(unsafe { &mut *out });
    }
}
