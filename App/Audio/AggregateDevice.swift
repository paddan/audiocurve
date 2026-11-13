import AudioCurveCore
import CoreAudio
import Foundation

/// A private aggregate device: the tap as input, the output device as main subdevice.
final class AggregateDevice {
    let id: AudioObjectID
    private var destroyed = false

    init(outputUID: String, tapUID: String) throws {
        let description: [String: Any] = [
            kAudioAggregateDeviceNameKey: "AudioCurve",
            kAudioAggregateDeviceUIDKey: OutputDeviceMonitor.routeUIDPrefix + UUID().uuidString,
            kAudioAggregateDeviceMainSubDeviceKey: outputUID,
            kAudioAggregateDeviceIsPrivateKey: true,
            kAudioAggregateDeviceIsStackedKey: false,
            kAudioAggregateDeviceTapAutoStartKey: true,
            kAudioAggregateDeviceSubDeviceListKey: [[kAudioSubDeviceUIDKey: outputUID]],
            kAudioAggregateDeviceTapListKey: [[kAudioSubTapDriftCompensationKey: true, kAudioSubTapUIDKey: tapUID]],
        ]
        var device = AudioObjectID(kAudioObjectUnknown)
        try check(AudioHardwareCreateAggregateDevice(description as CFDictionary, &device), "AudioHardwareCreateAggregateDevice")
        id = device
    }

    var sampleRate: Double { (try? HAL.get(id, kAudioDevicePropertyNominalSampleRate, Float64(0))) ?? 0 }

    /// Sets the IO buffer to the smallest size the device accepts that is at least `minimum` frames.
    /// Returns the size in effect.
    @discardableResult
    func setBufferFrames(atLeast minimum: UInt32) -> UInt32 {
        if let range = try? HAL.get(id, kAudioDevicePropertyBufferFrameSizeRange, AudioValueRange()) {
            let frames = UInt32(max(Double(minimum), range.mMinimum).rounded(.up))
            try? HAL.set(id, kAudioDevicePropertyBufferFrameSize, min(frames, UInt32(range.mMaximum)))
        }
        return (try? HAL.get(id, kAudioDevicePropertyBufferFrameSize, UInt32(0))) ?? 0
    }

    func destroy() {
        guard !destroyed else { return }
        destroyed = true
        AudioHardwareDestroyAggregateDevice(id)
    }

    deinit { destroy() }
}
