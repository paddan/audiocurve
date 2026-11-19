//! An output device's hardware volume for the hearing test (Swift `DeviceVolume.swift`):
//! `kAudioDevicePropertyVolumeScalar` on the output scope, per element.

use std::collections::BTreeMap;
use std::mem::size_of;
use std::ptr::{self, NonNull};

use audiocurve_core::volume::{ElementRestore, VolumeSnapshot};
use objc2_core_audio::{
    AudioObjectGetPropertyData, AudioObjectHasProperty, AudioObjectIsPropertySettable,
    AudioObjectPropertyAddress, AudioObjectSetPropertyData, kAudioDevicePropertyMute,
    kAudioDevicePropertyVolumeScalar, kAudioObjectPropertyElementMain,
    kAudioObjectPropertyScopeOutput,
};

use super::hal::{self, ObjectId};
use crate::platform::Volume;

/// Main element and the first two channels.
const ELEMENTS: [u32; 3] = [kAudioObjectPropertyElementMain, 1, 2];
/// A raised volume counts as max from this scalar up.
const MAX_THRESHOLD: f32 = 0.99;

pub struct MacVolume;

fn addr(selector: u32, element: u32) -> AudioObjectPropertyAddress {
    AudioObjectPropertyAddress {
        mSelector: selector,
        mScope: kAudioObjectPropertyScopeOutput,
        mElement: element,
    }
}

fn has(id: ObjectId, selector: u32, element: u32) -> bool {
    let mut a = addr(selector, element);
    // SAFETY: `a` is a valid address.
    unsafe { AudioObjectHasProperty(id, NonNull::from(&mut a)) }
}

fn is_settable_element(id: ObjectId, element: u32) -> bool {
    if !has(id, kAudioDevicePropertyVolumeScalar, element) {
        return false;
    }
    let mut a = addr(kAudioDevicePropertyVolumeScalar, element);
    let mut settable = 0u8;
    // SAFETY: valid address and a `Boolean` out-parameter.
    let status = unsafe {
        AudioObjectIsPropertySettable(id, NonNull::from(&mut a), NonNull::from(&mut settable))
    };
    status == 0 && settable != 0
}

fn settable_elements(id: ObjectId) -> Vec<u32> {
    ELEMENTS
        .into_iter()
        .filter(|&e| is_settable_element(id, e))
        .collect()
}

fn get_u32(id: ObjectId, selector: u32, element: u32) -> Option<u32> {
    let mut a = addr(selector, element);
    let mut v = 0u32;
    let mut size = size_of::<u32>() as u32;
    // SAFETY: the property is a `UInt32`.
    let status = unsafe {
        AudioObjectGetPropertyData(
            id,
            NonNull::from(&mut a),
            0,
            ptr::null(),
            NonNull::from(&mut size),
            NonNull::from(&mut v).cast(),
        )
    };
    (status == 0).then_some(v)
}

fn scalar(id: ObjectId, element: u32) -> Option<f32> {
    let mut a = addr(kAudioDevicePropertyVolumeScalar, element);
    let mut v = 0f32;
    let mut size = size_of::<f32>() as u32;
    // SAFETY: the property is a `Float32`.
    let status = unsafe {
        AudioObjectGetPropertyData(
            id,
            NonNull::from(&mut a),
            0,
            ptr::null(),
            NonNull::from(&mut size),
            NonNull::from(&mut v).cast(),
        )
    };
    (status == 0 && v.is_finite()).then_some(v)
}

fn set_scalar(id: ObjectId, element: u32, value: f32) -> bool {
    let mut a = addr(kAudioDevicePropertyVolumeScalar, element);
    let mut v = value;
    // SAFETY: the property is a `Float32`.
    let status = unsafe {
        AudioObjectSetPropertyData(
            id,
            NonNull::from(&mut a),
            0,
            ptr::null(),
            size_of::<f32>() as u32,
            NonNull::from(&mut v).cast(),
        )
    };
    status == 0
}

impl Volume for MacVolume {
    /// The scalar of every settable element, or None if the device is missing or has none.
    fn snapshot(&self, uid: &str) -> Option<VolumeSnapshot> {
        let id = hal::device_by_uid(uid)?;
        let scalars: BTreeMap<u32, f32> = settable_elements(id)
            .into_iter()
            .filter_map(|e| scalar(id, e).map(|v| (e, v)))
            .collect();
        (!scalars.is_empty()).then(|| VolumeSnapshot {
            device_uid: uid.to_string(),
            scalars,
            failed_attempts: 0,
        })
    }

    /// Raises every element of the snapshot to 1.0 and reads them back; an element whose volume
    /// could not be read (so could not be restored) is never raised. False if the device is
    /// missing or any element reads back below the threshold.
    fn set_max(&mut self, uid: &str) -> bool {
        let Some(id) = hal::device_by_uid(uid) else {
            return false;
        };
        // Only elements that can be read back, hence restored, are raised.
        let elements: Vec<u32> = settable_elements(id)
            .into_iter()
            .filter(|&e| scalar(id, e).is_some())
            .collect();
        if elements.is_empty() {
            return false;
        }
        let mut ok = true;
        for &e in &elements {
            ok &= set_scalar(id, e, 1.0);
        }
        for &e in &elements {
            ok &= scalar(id, e).unwrap_or(0.0) >= MAX_THRESHOLD;
        }
        if !ok {
            eprintln!("AudioCurve: could not set the volume of {uid} to max");
        }
        ok
    }

    /// Writes the saved scalars back. A missing device reports every element `Failed`, so the
    /// snapshot stays pending and is retried rather than lost.
    fn restore(&mut self, s: &VolumeSnapshot) -> BTreeMap<u32, ElementRestore> {
        let Some(id) = hal::device_by_uid(&s.device_uid) else {
            return s
                .scalars
                .keys()
                .map(|&e| (e, ElementRestore::Failed))
                .collect();
        };
        let writable = settable_elements(id);
        let outcomes: BTreeMap<u32, ElementRestore> = s
            .scalars
            .iter()
            .map(|(&e, &v)| {
                let outcome = if !writable.contains(&e) || !v.is_finite() {
                    ElementRestore::NotWritable
                } else if set_scalar(id, e, v.clamp(0.0, 1.0)) {
                    ElementRestore::Restored
                } else {
                    ElementRestore::Failed
                };
                (e, outcome)
            })
            .collect();
        let left = s.remaining(&outcomes);
        let dropped: Vec<String> = outcomes
            .iter()
            .filter(|&(_, &o)| {
                o == ElementRestore::NotWritable || (o == ElementRestore::Failed && left.is_none())
            })
            .map(|(e, _)| e.to_string())
            .collect();
        if !dropped.is_empty() {
            eprintln!(
                "AudioCurve: gave up restoring the volume of {}, elements {}",
                s.device_uid,
                dropped.join(", ")
            );
        }
        outcomes
    }

    /// True if `kAudioDevicePropertyMute` reads 1 on the output scope of any element.
    fn is_muted(&self, uid: &str) -> bool {
        let Some(id) = hal::device_by_uid(uid) else {
            return false;
        };
        ELEMENTS.into_iter().any(|e| {
            has(id, kAudioDevicePropertyMute, e)
                && get_u32(id, kAudioDevicePropertyMute, e) == Some(1)
        })
    }

    /// Can the volume be set (and restored)? As in Swift: some element is settable and readable.
    fn is_settable(&self, uid: &str) -> bool {
        self.snapshot(uid).is_some()
    }
}
