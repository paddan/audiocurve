import AudioCurveCore
import CoreAudio
import Foundation

/// Reads, maximises and restores an output device's hardware volume
/// (`kAudioDevicePropertyVolumeScalar`, output scope, per element) for the hearing test.
struct DeviceVolume {
    /// Main element and the first two channels.
    static let elements: [UInt32] = [kAudioObjectPropertyElementMain, 1, 2]
    /// A raised volume counts as max from this scalar up.
    static let maxThreshold: Float32 = 0.99

    /// The scalar of every settable element, or nil if the device is missing or has none.
    static func read(uid: String) -> VolumeSnapshot? {
        guard let id = HAL.device(uid: uid) else { return nil }
        var scalars: [UInt32: Float32] = [:]
        for e in settableElements(id) {
            if let v = scalar(id, e) { scalars[e] = v }
        }
        return scalars.isEmpty ? nil : VolumeSnapshot(deviceUID: uid, scalars: scalars)
    }

    /// Sets the elements saved in `s` to 1.0 and reads them back; an element whose volume could
    /// not be read (so could not be restored) is never raised. False if the device is missing,
    /// `s` has no element, or any of them reads back below `maxThreshold`.
    static func setMax(_ s: VolumeSnapshot) -> Bool {
        guard let id = HAL.device(uid: s.deviceUID), !s.scalars.isEmpty else { return false }
        var ok = true
        for e in s.scalars.keys {
            if !setScalar(id, e, 1.0) { ok = false }
        }
        for e in s.scalars.keys where (scalar(id, e) ?? 0) < maxThreshold { ok = false }
        if !ok { NSLog("AudioCurve: could not set the volume of %@ to max", s.deviceUID) }
        return ok
    }

    /// Writes the saved scalars back. Returns what is still to restore (`VolumeSnapshot.remaining`),
    /// or nil when done; an unchanged `s` if the device is missing. Elements given up are logged
    /// once, when they are dropped.
    static func restore(_ s: VolumeSnapshot) -> VolumeSnapshot? {
        guard let id = HAL.device(uid: s.deviceUID) else { return s }
        let writable = Set(settableElements(id))
        var outcomes: [UInt32: VolumeSnapshot.ElementRestore] = [:]
        for (e, v) in s.scalars {
            if !writable.contains(e) || !v.isFinite {
                outcomes[e] = .notWritable
            } else {
                outcomes[e] = setScalar(id, e, min(max(v, 0), 1)) ? .restored : .failed
            }
        }
        let left = s.remaining(after: outcomes)
        let dropped = s.scalars.keys.filter { outcomes[$0] == .notWritable || (outcomes[$0] == .failed && left == nil) }
        if !dropped.isEmpty {
            NSLog("AudioCurve: gave up restoring the volume of %@, elements %@", s.deviceUID,
                  dropped.sorted().map(String.init).joined(separator: ", "))
        }
        return left
    }

    /// True if `kAudioDevicePropertyMute` reads 1 on the output scope of any element.
    static func isMuted(uid: String) -> Bool {
        guard let id = HAL.device(uid: uid) else { return false }
        return elements.contains { e in
            var a = AudioObjectPropertyAddress(mSelector: kAudioDevicePropertyMute,
                                               mScope: kAudioDevicePropertyScopeOutput, mElement: e)
            guard AudioObjectHasProperty(id, &a) else { return false }
            var v: UInt32 = 0
            var size = UInt32(MemoryLayout<UInt32>.size)
            return AudioObjectGetPropertyData(id, &a, 0, nil, &size, &v) == noErr && v == 1
        }
    }

    private static func address(_ element: UInt32) -> AudioObjectPropertyAddress {
        AudioObjectPropertyAddress(mSelector: kAudioDevicePropertyVolumeScalar,
                                   mScope: kAudioDevicePropertyScopeOutput, mElement: element)
    }

    private static func settableElements(_ id: AudioObjectID) -> [UInt32] {
        elements.filter { e in
            var a = address(e)
            guard AudioObjectHasProperty(id, &a) else { return false }
            var settable: DarwinBoolean = false
            return AudioObjectIsPropertySettable(id, &a, &settable) == noErr && settable.boolValue
        }
    }

    private static func scalar(_ id: AudioObjectID, _ element: UInt32) -> Float32? {
        var a = address(element)
        var v: Float32 = 0
        var size = UInt32(MemoryLayout<Float32>.size)
        guard AudioObjectGetPropertyData(id, &a, 0, nil, &size, &v) == noErr, v.isFinite else { return nil }
        return v
    }

    private static func setScalar(_ id: AudioObjectID, _ element: UInt32, _ value: Float32) -> Bool {
        var a = address(element)
        var v = value
        return AudioObjectSetPropertyData(id, &a, 0, nil, UInt32(MemoryLayout<Float32>.size), &v) == noErr
    }
}
