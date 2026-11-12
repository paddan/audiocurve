import Foundation

public struct PluginState: Codable, Equatable, Sendable {
    public static let key = "audiocurve.state"
    public static let currentVersion = 1

    public var version: Int
    public var profile: HearingProfile?

    public init(version: Int, profile: HearingProfile?) {
        self.version = version
        self.profile = profile
    }

    public func encoded() -> Data {
        (try? JSONEncoder().encode(self)) ?? Data()
    }

    /// Never throws. Missing, corrupt, unknown-version or invalid data yields `profile == nil`.
    public static func decode(_ data: Data?) -> PluginState {
        let empty = PluginState(version: currentVersion, profile: nil)
        guard let data, let state = try? JSONDecoder().decode(PluginState.self, from: data) else { return empty }
        guard state.version == currentVersion else { return empty }
        guard let profile = state.profile else { return empty }
        guard (try? profile.validate()) != nil else { return empty }
        return state
    }
}

extension PluginState {
    /// Which profile an AU should hold after a host restores its state: a host-supplied profile wins,
    /// but an empty or foreign state must not wipe the one we already have (e.g. the autosaved copy).
    public static func resolve(hostProfile: HearingProfile?, current: HearingProfile?) -> HearingProfile? {
        hostProfile ?? current
    }
}
