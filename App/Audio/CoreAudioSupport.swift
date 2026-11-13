import CoreAudio
import Foundation

struct CoreAudioError: Error, CustomStringConvertible {
    let status: OSStatus
    let call: String
    var description: String { "\(call) failed (\(status))" }
}

@inline(__always)
func check(_ status: OSStatus, _ call: String) throws {
    if status != noErr { throw CoreAudioError(status: status, call: call) }
}

enum HAL {
    static func address(_ selector: AudioObjectPropertySelector,
                        _ scope: AudioObjectPropertyScope = kAudioObjectPropertyScopeGlobal) -> AudioObjectPropertyAddress {
        AudioObjectPropertyAddress(mSelector: selector, mScope: scope, mElement: kAudioObjectPropertyElementMain)
    }

    static func get<T: BitwiseCopyable>(_ object: AudioObjectID, _ selector: AudioObjectPropertySelector, _ initial: T,
                       scope: AudioObjectPropertyScope = kAudioObjectPropertyScopeGlobal) throws -> T {
        var a = address(selector, scope)
        var value = initial
        var size = UInt32(MemoryLayout<T>.size)
        try check(AudioObjectGetPropertyData(object, &a, 0, nil, &size, &value), "get \(selector)")
        return value
    }

    static func set<T: BitwiseCopyable>(_ object: AudioObjectID, _ selector: AudioObjectPropertySelector, _ value: T) throws {
        var a = address(selector)
        var v = value
        try check(AudioObjectSetPropertyData(object, &a, 0, nil, UInt32(MemoryLayout<T>.size), &v), "set \(selector)")
    }

    /// The device with this UID, or nil.
    static func device(uid: String) -> AudioObjectID? {
        var a = address(kAudioHardwarePropertyTranslateUIDToDevice)
        var cfUID = uid as CFString
        var id = AudioObjectID(kAudioObjectUnknown)
        var size = UInt32(MemoryLayout<AudioObjectID>.size)
        let st = withUnsafePointer(to: &cfUID) {
            AudioObjectGetPropertyData(AudioObjectID(kAudioObjectSystemObject), &a,
                                       UInt32(MemoryLayout<CFString>.size), $0, &size, &id)
        }
        return st == noErr && id != kAudioObjectUnknown ? id : nil
    }

    /// The HAL process object for a pid, or nil.
    static func processObject(pid: pid_t) -> AudioObjectID? {
        var a = address(kAudioHardwarePropertyTranslatePIDToProcessObject)
        var p = pid
        var id = AudioObjectID(kAudioObjectUnknown)
        var size = UInt32(MemoryLayout<AudioObjectID>.size)
        let st = AudioObjectGetPropertyData(AudioObjectID(kAudioObjectSystemObject), &a,
                                            UInt32(MemoryLayout<pid_t>.size), &p, &size, &id)
        return st == noErr && id != kAudioObjectUnknown ? id : nil
    }
}

extension HAL {
    /// Is any process other than this one playing audio right now?
    static func othersPlaying() -> Bool {
        var a = address(kAudioHardwarePropertyProcessObjectList)
        var size: UInt32 = 0
        let system = AudioObjectID(kAudioObjectSystemObject)
        guard AudioObjectGetPropertyDataSize(system, &a, 0, nil, &size) == noErr, size > 0 else { return false }
        var ids = [AudioObjectID](repeating: 0, count: Int(size) / MemoryLayout<AudioObjectID>.size)
        guard AudioObjectGetPropertyData(system, &a, 0, nil, &size, &ids) == noErr else { return false }
        let me = processObject(pid: getpid())
        return ids.contains { id in
            id != me && ((try? get(id, kAudioProcessPropertyIsRunningOutput, UInt32(0))) ?? 0) != 0
        }
    }
}
