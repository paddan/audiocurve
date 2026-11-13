import CoreAudio
import Foundation

/// Watches the system's default output device and the list of output devices (Core Audio HAL).
/// Listener callbacks run on the main queue; `onChange` is called there after either changes.
public final class OutputDeviceMonitor: @unchecked Sendable {
    /// Called on the main queue when the default output or the device list changed.
    public var onChange: (() -> Void)?

    private var listeners: [(AudioObjectPropertyAddress, AudioObjectPropertyListenerBlock)] = []

    public init() {
        for selector in [kAudioHardwarePropertyDefaultOutputDevice, kAudioHardwarePropertyDevices] {
            var address = AudioObjectPropertyAddress(mSelector: selector,
                                                     mScope: kAudioObjectPropertyScopeGlobal,
                                                     mElement: kAudioObjectPropertyElementMain)
            let block: AudioObjectPropertyListenerBlock = { [weak self] _, _ in self?.onChange?() }
            if AudioObjectAddPropertyListenerBlock(AudioObjectID(kAudioObjectSystemObject), &address,
                                                   DispatchQueue.main, block) == noErr {
                listeners.append((address, block))
            }
        }
    }

    deinit {
        for (address, block) in listeners {
            var a = address
            AudioObjectRemovePropertyListenerBlock(AudioObjectID(kAudioObjectSystemObject), &a, DispatchQueue.main, block)
        }
    }

    /// The current default output device, or nil if it cannot be read (e.g. no access).
    public var current: OutputDevice? {
        var id = AudioObjectID(kAudioObjectUnknown)
        var size = UInt32(MemoryLayout<AudioObjectID>.size)
        var address = AudioObjectPropertyAddress(mSelector: kAudioHardwarePropertyDefaultOutputDevice,
                                                 mScope: kAudioObjectPropertyScopeGlobal,
                                                 mElement: kAudioObjectPropertyElementMain)
        guard AudioObjectGetPropertyData(AudioObjectID(kAudioObjectSystemObject), &address, 0, nil, &size, &id) == noErr,
              id != kAudioObjectUnknown else { return nil }
        return Self.device(id)
    }

    /// All devices that have output streams, sorted by name.
    public var available: [OutputDevice] {
        var address = AudioObjectPropertyAddress(mSelector: kAudioHardwarePropertyDevices,
                                                 mScope: kAudioObjectPropertyScopeGlobal,
                                                 mElement: kAudioObjectPropertyElementMain)
        var size: UInt32 = 0
        let system = AudioObjectID(kAudioObjectSystemObject)
        guard AudioObjectGetPropertyDataSize(system, &address, 0, nil, &size) == noErr, size > 0 else { return [] }
        var ids = [AudioObjectID](repeating: 0, count: Int(size) / MemoryLayout<AudioObjectID>.size)
        guard AudioObjectGetPropertyData(system, &address, 0, nil, &size, &ids) == noErr else { return [] }
        return ids.filter(Self.hasOutput).compactMap(Self.device)
            .sorted { $0.name.localizedStandardCompare($1.name) == .orderedAscending }
    }

    private static func hasOutput(_ id: AudioObjectID) -> Bool {
        var address = AudioObjectPropertyAddress(mSelector: kAudioDevicePropertyStreams,
                                                 mScope: kAudioObjectPropertyScopeOutput,
                                                 mElement: kAudioObjectPropertyElementMain)
        var size: UInt32 = 0
        return AudioObjectGetPropertyDataSize(id, &address, 0, nil, &size) == noErr && size > 0
    }

    private static func device(_ id: AudioObjectID) -> OutputDevice? {
        guard let uid = string(id, kAudioDevicePropertyDeviceUID) else { return nil }
        return OutputDevice(uid: uid, name: string(id, kAudioObjectPropertyName) ?? uid)
    }

    private static func string(_ id: AudioObjectID, _ selector: AudioObjectPropertySelector) -> String? {
        var address = AudioObjectPropertyAddress(mSelector: selector,
                                                 mScope: kAudioObjectPropertyScopeGlobal,
                                                 mElement: kAudioObjectPropertyElementMain)
        var value: Unmanaged<CFString>?
        var size = UInt32(MemoryLayout<Unmanaged<CFString>?>.size)
        guard AudioObjectGetPropertyData(id, &address, 0, nil, &size, &value) == noErr,
              let s = value?.takeRetainedValue() else { return nil }
        return s as String
    }
}
