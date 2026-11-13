import Foundation

/// A plugin instance's "I am here" note, written to the shared store every `interval` so that
/// the AudioCurve app can tell when another host also processes the sound.
public struct InstanceHeartbeat: Codable, Sendable, Equatable {
    public var pid: Int32
    /// The host's bundle ID, or "unknown" until the host identifies itself.
    public var hostBundleID: String
    /// Processing is on for the current output.
    public var active: Bool
    public var time: Date

    public static let interval: TimeInterval = 2
    public static let staleAfter: TimeInterval = 6

    public init(pid: Int32, hostBundleID: String, active: Bool, time: Date) {
        self.pid = pid
        self.hostBundleID = hostBundleID
        self.active = active
        self.time = time
    }

    /// Active beats newer than `staleAfter`, other than `pid`, sorted by host bundle ID.
    public static func live(_ beats: [InstanceHeartbeat], now: Date, excludingPID pid: Int32) -> [InstanceHeartbeat] {
        beats.filter { $0.pid != pid && $0.active && now.timeIntervalSince($0.time) <= staleAfter }
            .sorted { ($0.hostBundleID, $0.pid) < ($1.hostBundleID, $1.pid) }
    }
}
