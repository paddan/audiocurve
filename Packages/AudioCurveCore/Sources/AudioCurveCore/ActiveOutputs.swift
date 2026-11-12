import Foundation

/// An audio output device as the user saw it (Core Audio UID plus its name at the time).
public struct OutputDevice: Codable, Hashable, Identifiable, Sendable {
    public var uid: String
    public var name: String
    public var id: String { uid }

    public init(uid: String, name: String) {
        self.uid = uid
        self.name = name
    }
}

/// The output devices processing is enabled for. Empty means every device (the default).
public struct ActiveOutputs: Codable, Equatable, Sendable {
    public var devices: [OutputDevice]

    public init(devices: [OutputDevice] = []) { self.devices = devices }

    public var isAll: Bool { devices.isEmpty }

    public func contains(_ uid: String) -> Bool { devices.contains { $0.uid == uid } }

    /// Should the plugin process audio while `currentUID` is the system output?
    /// Unknown output (nil) or no selection keeps processing on, so a lookup failure never
    /// silently turns the hearing compensation off.
    public func isActive(currentUID: String?) -> Bool {
        guard !devices.isEmpty, let uid = currentUID else { return true }
        return contains(uid)
    }

    /// Adds or removes `device`; adding refreshes the stored name.
    public mutating func toggle(_ device: OutputDevice) {
        if let i = devices.firstIndex(where: { $0.uid == device.uid }) {
            devices.remove(at: i)
        } else {
            devices.append(device)
        }
    }

    /// Devices to offer in a menu: the ones currently available, then remembered ones that are not.
    public func menuDevices(available: [OutputDevice]) -> [OutputDevice] {
        available + devices.filter { d in !available.contains { $0.uid == d.uid } }
    }
}
