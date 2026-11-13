import CoreAudio
import Foundation

/// A private stereo tap on every process except the given ones, muting what it taps.
final class ProcessTap {
    let id: AudioObjectID
    let uid: String
    private var destroyed = false

    init(excluding pids: [pid_t]) throws {
        let objects = pids.compactMap { HAL.processObject(pid: $0) }
        let desc = CATapDescription(stereoGlobalTapButExcludeProcesses: objects)
        desc.name = "AudioCurve"
        desc.isPrivate = true
        desc.muteBehavior = .mutedWhenTapped
        var tap = AudioObjectID(kAudioObjectUnknown)
        try check(AudioHardwareCreateProcessTap(desc, &tap), "AudioHardwareCreateProcessTap")
        id = tap
        uid = desc.uuid.uuidString
    }

    /// Destroying the tap unmutes the tapped processes.
    func destroy() {
        guard !destroyed else { return }
        destroyed = true
        AudioHardwareDestroyProcessTap(id)
    }

    deinit { destroy() }
}
