//! Thin safe wrappers over the Core Audio HAL (Swift `CoreAudioSupport.swift` and
//! `OutputDeviceMonitor.swift`).

use std::ffi::c_void;
use std::fmt;
use std::mem::size_of;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::ptr::{self, NonNull};

use audiocurve_core::outputs::OutputDevice;
use block2::RcBlock;
use dispatch2::DispatchQueue;
use objc2_core_audio::{
    AudioObjectAddPropertyListenerBlock, AudioObjectGetPropertyData,
    AudioObjectGetPropertyDataSize, AudioObjectPropertyAddress, AudioObjectPropertyScope,
    AudioObjectRemovePropertyListenerBlock, AudioObjectSetPropertyData,
    kAudioDevicePropertyDeviceUID, kAudioDevicePropertyNominalSampleRate,
    kAudioDevicePropertyStreams, kAudioHardwarePropertyDefaultOutputDevice,
    kAudioHardwarePropertyDevices, kAudioHardwarePropertyProcessObjectList,
    kAudioHardwarePropertyTranslatePIDToProcessObject, kAudioHardwarePropertyTranslateUIDToDevice,
    kAudioObjectPropertyElementMain, kAudioObjectPropertyName, kAudioObjectPropertyScopeGlobal,
    kAudioObjectPropertyScopeOutput, kAudioObjectSystemObject, kAudioObjectUnknown,
    kAudioProcessPropertyIsRunningOutput,
};
use objc2_core_foundation::{CFRetained, CFString};

pub type ObjectId = u32;

/// The system object, which owns the device and process lists.
pub const SYSTEM: ObjectId = kAudioObjectSystemObject as ObjectId;

/// The UID prefix of AudioCurve's own private aggregate devices.
pub const ROUTE_UID_PREFIX: &str = "se.lindefors.audiocurve.route.";

/// A HAL call that returned a non-zero `OSStatus`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HalError {
    pub status: i32,
    pub call: &'static str,
}

impl fmt::Display for HalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} failed ({})", self.call, self.status)
    }
}

impl std::error::Error for HalError {}

/// `Ok(())` for `noErr`, else a `HalError` naming the call.
pub fn check(status: i32, call: &'static str) -> Result<(), HalError> {
    if status == 0 {
        Ok(())
    } else {
        Err(HalError { status, call })
    }
}

pub fn address(selector: u32, scope: AudioObjectPropertyScope) -> AudioObjectPropertyAddress {
    AudioObjectPropertyAddress {
        mSelector: selector,
        mScope: scope,
        mElement: kAudioObjectPropertyElementMain,
    }
}

/// Reads a fixed-size property, with optional qualifier data.
///
/// # Safety
/// `T` must be the property's exact plain-data type, and `Q` the qualifier's.
unsafe fn get_with<T: Copy, Q>(
    object: ObjectId,
    selector: u32,
    scope: AudioObjectPropertyScope,
    qualifier: Option<&Q>,
    initial: T,
) -> Result<T, HalError> {
    let mut a = address(selector, scope);
    let mut value = initial;
    let mut size = size_of::<T>() as u32;
    let (q_size, q_ptr) = match qualifier {
        Some(q) => (size_of::<Q>() as u32, ptr::from_ref(q).cast::<c_void>()),
        None => (0, ptr::null()),
    };
    // SAFETY: `value` is `size` bytes of `T`, the type the caller vouches for.
    let status = unsafe {
        AudioObjectGetPropertyData(
            object,
            NonNull::from(&mut a),
            q_size,
            q_ptr,
            NonNull::from(&mut size),
            NonNull::from(&mut value).cast(),
        )
    };
    check(status, "AudioObjectGetPropertyData")?;
    Ok(value)
}

/// Reads a fixed-size global property.
///
/// # Safety
/// `T` must be the property's exact plain-data type.
pub unsafe fn get<T: Copy>(object: ObjectId, selector: u32, initial: T) -> Result<T, HalError> {
    // SAFETY: forwarded from the caller.
    unsafe {
        get_with::<T, ()>(
            object,
            selector,
            kAudioObjectPropertyScopeGlobal,
            None,
            initial,
        )
    }
}

/// Writes a fixed-size global property.
///
/// # Safety
/// `T` must be the property's exact plain-data type.
pub unsafe fn set<T: Copy>(object: ObjectId, selector: u32, value: T) -> Result<(), HalError> {
    let mut a = address(selector, kAudioObjectPropertyScopeGlobal);
    let mut value = value;
    // SAFETY: `value` is `size_of::<T>()` bytes of the type the caller vouches for.
    let status = unsafe {
        AudioObjectSetPropertyData(
            object,
            NonNull::from(&mut a),
            0,
            ptr::null(),
            size_of::<T>() as u32,
            NonNull::from(&mut value).cast(),
        )
    };
    check(status, "AudioObjectSetPropertyData")
}

/// Reads a global property that is an array of object IDs.
fn get_ids(object: ObjectId, selector: u32) -> Vec<ObjectId> {
    let mut a = address(selector, kAudioObjectPropertyScopeGlobal);
    let mut size = 0u32;
    // SAFETY: plain out-parameters.
    let status = unsafe {
        AudioObjectGetPropertyDataSize(
            object,
            NonNull::from(&mut a),
            0,
            ptr::null(),
            NonNull::from(&mut size),
        )
    };
    if status != 0 || size == 0 {
        return Vec::new();
    }
    let mut ids: Vec<ObjectId> = vec![0; size as usize / size_of::<ObjectId>()];
    // SAFETY: `ids` holds `size` bytes of `AudioObjectID`s.
    let status = unsafe {
        AudioObjectGetPropertyData(
            object,
            NonNull::from(&mut a),
            0,
            ptr::null(),
            NonNull::from(&mut size),
            NonNull::new(ids.as_mut_ptr()).unwrap().cast(),
        )
    };
    if status != 0 {
        return Vec::new();
    }
    ids.truncate(size as usize / size_of::<ObjectId>());
    ids
}

/// Reads a global property that is a `CFStringRef` the caller owns.
fn get_string(object: ObjectId, selector: u32) -> Option<String> {
    // SAFETY: the property is a `CFStringRef`.
    let raw = unsafe { get::<*const CFString>(object, selector, ptr::null()) }.ok()?;
    // SAFETY: HAL string properties return a +1 reference, which we now own.
    let s = unsafe { CFRetained::from_raw(NonNull::new(raw.cast_mut())?) };
    Some(s.to_string())
}

/// The system's default output device.
pub fn default_output() -> Option<ObjectId> {
    // SAFETY: the property is an `AudioObjectID`.
    let id = unsafe { get(SYSTEM, kAudioHardwarePropertyDefaultOutputDevice, 0u32) }.ok()?;
    (id != kAudioObjectUnknown).then_some(id)
}

pub fn device_uid(id: ObjectId) -> Option<String> {
    get_string(id, kAudioDevicePropertyDeviceUID)
}

pub fn device_name(id: ObjectId) -> Option<String> {
    get_string(id, kAudioObjectPropertyName)
}

/// The device with this UID.
pub fn device_by_uid(uid: &str) -> Option<ObjectId> {
    let cf = CFString::from_str(uid);
    let cf_ref: *const CFString = &*cf;
    // SAFETY: the qualifier is a `CFStringRef`, the result an `AudioObjectID`.
    let id = unsafe {
        get_with(
            SYSTEM,
            kAudioHardwarePropertyTranslateUIDToDevice,
            kAudioObjectPropertyScopeGlobal,
            Some(&cf_ref),
            0u32,
        )
    }
    .ok()?;
    (id != kAudioObjectUnknown).then_some(id)
}

fn has_output(id: ObjectId) -> bool {
    let mut a = address(kAudioDevicePropertyStreams, kAudioObjectPropertyScopeOutput);
    let mut size = 0u32;
    // SAFETY: plain out-parameters.
    let status = unsafe {
        AudioObjectGetPropertyDataSize(
            id,
            NonNull::from(&mut a),
            0,
            ptr::null(),
            NonNull::from(&mut size),
        )
    };
    status == 0 && size > 0
}

/// The device as an `OutputDevice`; its name falls back to its UID.
pub fn output_device(id: ObjectId) -> Option<OutputDevice> {
    let uid = device_uid(id)?;
    let name = device_name(id).unwrap_or_else(|| uid.clone());
    Some(OutputDevice { uid, name })
}

/// Every device the HAL lists to this process, AudioCurve's own private routes included.
pub fn devices() -> Vec<ObjectId> {
    get_ids(SYSTEM, kAudioHardwarePropertyDevices)
}

/// Devices with output streams, minus AudioCurve's own routes, in HAL order.
pub fn output_devices() -> Vec<OutputDevice> {
    devices()
        .into_iter()
        .filter(|&id| has_output(id))
        .filter_map(output_device)
        .filter(|d| !is_own_route(&d.uid))
        .collect()
}

/// Is this one of AudioCurve's own route devices (never offered as an output)?
pub fn is_own_route(uid: &str) -> bool {
    uid.starts_with(ROUTE_UID_PREFIX)
}

pub fn nominal_sample_rate(id: ObjectId) -> Option<f64> {
    // SAFETY: the property is a `Float64`.
    let rate = unsafe { get(id, kAudioDevicePropertyNominalSampleRate, 0f64) }.ok()?;
    (rate > 0.0 && rate.is_finite()).then_some(rate)
}

/// The HAL process object for a pid.
pub fn process_object(pid: i32) -> Option<ObjectId> {
    // SAFETY: the qualifier is a `pid_t`, the result an `AudioObjectID`.
    let id = unsafe {
        get_with(
            SYSTEM,
            kAudioHardwarePropertyTranslatePIDToProcessObject,
            kAudioObjectPropertyScopeGlobal,
            Some(&pid),
            0u32,
        )
    }
    .ok()?;
    (id != kAudioObjectUnknown).then_some(id)
}

/// Is any process other than this one playing audio right now?
pub fn others_playing() -> bool {
    let me = process_object(std::process::id() as i32);
    get_ids(SYSTEM, kAudioHardwarePropertyProcessObjectList)
        .into_iter()
        .filter(|&id| Some(id) != me)
        // SAFETY: the property is a `UInt32`.
        .any(|id| unsafe { get(id, kAudioProcessPropertyIsRunningOutput, 0u32) }.unwrap_or(0) != 0)
}

type ListenerBlock = RcBlock<dyn Fn(u32, NonNull<AudioObjectPropertyAddress>)>;

/// A property listener block on the main dispatch queue; removed when dropped.
pub struct Listener {
    object: ObjectId,
    address: AudioObjectPropertyAddress,
    block: ListenerBlock,
    registered: bool,
}

/// Calls `f` on the main queue whenever the object's global `selector` property changes.
pub fn listen(object: ObjectId, selector: u32, f: impl Fn() + Send + 'static) -> Listener {
    let block: ListenerBlock =
        RcBlock::new(move |_: u32, _: NonNull<AudioObjectPropertyAddress>| {
            // A panic must not unwind into Core Audio.
            let _ = catch_unwind(AssertUnwindSafe(&f));
        });
    let mut l = Listener {
        object,
        address: address(selector, kAudioObjectPropertyScopeGlobal),
        block,
        registered: false,
    };
    // SAFETY: the block stays alive in `l` until it is removed in `drop`.
    let status = unsafe {
        AudioObjectAddPropertyListenerBlock(
            l.object,
            NonNull::from(&mut l.address),
            Some(DispatchQueue::main()),
            l.block_ptr(),
        )
    };
    l.registered = status == 0;
    l
}

impl Listener {
    /// The HAL accepted the listener.
    pub fn is_registered(&self) -> bool {
        self.registered
    }

    fn block_ptr(&self) -> *mut block2::DynBlock<dyn Fn(u32, NonNull<AudioObjectPropertyAddress>)> {
        ptr::from_ref(&*self.block).cast_mut()
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        if !self.registered {
            return;
        }
        let block = self.block_ptr();
        // SAFETY: the same object, address, queue and block that were added.
        unsafe {
            AudioObjectRemovePropertyListenerBlock(
                self.object,
                NonNull::from(&mut self.address),
                Some(DispatchQueue::main()),
                block,
            );
        }
    }
}
