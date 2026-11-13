import Foundation

public struct ProfileSummary: Codable, Sendable, Equatable, Identifiable {
    public var id: UUID
    public var name: String
    public init(id: UUID, name: String) { self.id = id; self.name = name }
}

/// What the plugin tells its host through `fullState` (see `PluginChannel`).
public struct PluginStatus: Codable, Sendable, Equatable {
    public var profiles: [ProfileSummary]
    public var activeProfileID: UUID?
    public var activeOutputs: ActiveOutputs
    /// Host bundle IDs of other plugin instances processing right now ("unknown" if not identified).
    public var otherHosts: [String]

    public init(profiles: [ProfileSummary], activeProfileID: UUID?, activeOutputs: ActiveOutputs, otherHosts: [String]) {
        self.profiles = profiles
        self.activeProfileID = activeProfileID
        self.activeOutputs = activeOutputs
        self.otherHosts = otherHosts
    }
}

/// What a host asks the plugin to do through `fullState`.
public enum PluginCommand: Codable, Sendable, Equatable {
    case loadProfile(UUID)
    case setActiveOutputs(ActiveOutputs)
    /// The host's bundle ID, recorded in the instance heartbeat.
    case identifyHost(String)
}

/// The app ↔ plugin channel. `AUMessageChannel` returns empty replies across the process
/// boundary, but `fullState` crosses it both ways: the plugin's getter carries the status, and
/// a host sets a dictionary holding only a command.
public enum PluginChannel {
    public static let statusKey = "audiocurve.status"
    public static let commandKey = "audiocurve.command"

    public static func encode(_ status: PluginStatus) -> Data {
        (try? JSONEncoder().encode(status)) ?? Data()
    }

    /// nil if missing or undecodable.
    public static func status(from fullState: [String: Any]?) -> PluginStatus? {
        guard let data = fullState?[statusKey] as? Data else { return nil }
        return try? JSONDecoder().decode(PluginStatus.self, from: data)
    }

    public static func commandState(_ command: PluginCommand) -> [String: Any] {
        [commandKey: (try? JSONEncoder().encode(command)) ?? Data()]
    }

    /// nil if missing, unknown or undecodable.
    public static func command(from fullState: [String: Any]?) -> PluginCommand? {
        guard let data = fullState?[commandKey] as? Data else { return nil }
        return try? JSONDecoder().decode(PluginCommand.self, from: data)
    }
}
