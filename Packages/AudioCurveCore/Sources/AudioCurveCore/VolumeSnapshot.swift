import Foundation

/// An output device's hardware volume (scalar per settable element) saved before the
/// hearing test raises it to max, so it can be restored afterwards — also after a crash.
///
/// Invariant: a pending snapshot holds the user's original volume and is never replaced while it
/// exists; it only shrinks as elements are restored or given up, and is cleared when none remain.
public struct VolumeSnapshot: Codable, Equatable, Sendable {
    public var deviceUID: String
    /// Element (0 = main, 1, 2 = channels) → `kAudioDevicePropertyVolumeScalar`.
    public var scalars: [UInt32: Float32]
    /// Restore attempts in which some element's write failed; nil before the first.
    public var failedAttempts: Int?

    public init(deviceUID: String, scalars: [UInt32: Float32], failedAttempts: Int? = nil) {
        self.deviceUID = deviceUID
        self.scalars = scalars
        self.failedAttempts = failedAttempts
    }

    /// How writing one element back went.
    public enum ElementRestore: Equatable, Sendable {
        case restored
        /// The element is gone or no longer accepts writes: nothing to retry.
        case notWritable
        /// The write was refused; retried (a reconnected device may not be ready yet).
        case failed
    }

    /// Attempts with a failed write before the remaining elements are given up, so a write that
    /// keeps failing does not keep overriding the user's volume keys.
    public static let maxRestoreAttempts = 5

    /// What is still to restore after one attempt with these per-element outcomes, or nil when
    /// the restore is done: every element was restored, does not accept writes, or failed
    /// `maxRestoreAttempts` times. Elements missing from `outcomes` count as not writable.
    public func remaining(after outcomes: [UInt32: ElementRestore]) -> VolumeSnapshot? {
        let failed = scalars.filter { outcomes[$0.key] == .failed }
        guard !failed.isEmpty else { return nil }
        let attempts = (failedAttempts ?? 0) + 1
        guard attempts < Self.maxRestoreAttempts else { return nil }
        return VolumeSnapshot(deviceUID: deviceUID, scalars: failed, failedAttempts: attempts)
    }

    /// Holds a JSON array of snapshots, one per device. Before 2.2 shipped it held a single
    /// snapshot; that value is still read.
    public static let pendingKey = "hearingTest.pendingVolume"

    /// Records the volume to restore for `s.deviceUID`. Written before the volume is raised. If
    /// that device already has a pending snapshot, it is the original and its values are never
    /// replaced: only elements it lacks are added from `s`.
    public static func savePending(_ s: VolumeSnapshot, _ defaults: UserDefaults = .standard) {
        var all = pending(defaults)
        if let i = all.firstIndex(where: { $0.deviceUID == s.deviceUID }) {
            all[i].scalars.merge(s.scalars) { original, _ in original }
        } else {
            all.append(s)
        }
        write(all, defaults)
    }

    /// The pending snapshot of one device, or nil.
    public static func pending(uid: String, _ defaults: UserDefaults = .standard) -> VolumeSnapshot? {
        pending(defaults).first { $0.deviceUID == uid }
    }

    /// The volumes still waiting to be restored, at most one per device.
    public static func pending(_ defaults: UserDefaults = .standard) -> [VolumeSnapshot] {
        guard let data = defaults.data(forKey: pendingKey) else { return [] }
        if let all = try? JSONDecoder().decode([VolumeSnapshot].self, from: data) { return all }
        if let one = try? JSONDecoder().decode(VolumeSnapshot.self, from: data) { return [one] }
        return []
    }

    /// After a restore attempt: keeps what `remaining` says is left of that device's snapshot,
    /// or forgets it when nil.
    public static func updatePending(uid: String, remaining: VolumeSnapshot?, _ defaults: UserDefaults = .standard) {
        var all = pending(defaults)
        guard let i = all.firstIndex(where: { $0.deviceUID == uid }) else { return }
        if let r = remaining { all[i] = r } else { all.remove(at: i) }
        write(all, defaults)
    }

    /// Forgets the snapshot of one device; the others stay.
    public static func clearPending(uid: String, _ defaults: UserDefaults = .standard) {
        write(pending(defaults).filter { $0.deviceUID != uid }, defaults)
    }

    private static func write(_ all: [VolumeSnapshot], _ defaults: UserDefaults) {
        if all.isEmpty {
            defaults.removeObject(forKey: pendingKey)
        } else if let data = try? JSONEncoder().encode(all) {
            defaults.set(data, forKey: pendingKey)
        }
    }
}
