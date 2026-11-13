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

    /// Is `uid` ticked in the list? With no selection every device is.
    public func isChecked(_ uid: String) -> Bool { isAll || contains(uid) }

    /// Is `uid` the only ticked device? It cannot be unticked, since an empty selection means all.
    public func isLastChecked(_ uid: String, available: [OutputDevice]) -> Bool {
        let shown = menuDevices(available: available)
        return shown.filter { isChecked($0.uid) }.map(\.uid) == [uid]
    }

    /// Ticks or unticks `device` in the list of `available` devices (plus remembered ones).
    /// Unticking from "all" keeps every other device; ticking every device returns to "all";
    /// the last ticked device stays ticked.
    public mutating func setEnabled(_ device: OutputDevice, _ enabled: Bool, available: [OutputDevice]) {
        let shown = menuDevices(available: available)
        if enabled {
            guard !isChecked(device.uid) else { return }
            devices.append(device)
            if shown.allSatisfy({ contains($0.uid) }) { devices = [] }
        } else {
            guard isChecked(device.uid), !isLastChecked(device.uid, available: available) else { return }
            if isAll { devices = shown }
            devices.removeAll { $0.uid == device.uid }
        }
    }

    /// Devices to list: the ones currently available, then remembered ones that are not.
    public func menuDevices(available: [OutputDevice]) -> [OutputDevice] {
        available + devices.filter { d in !available.contains { $0.uid == d.uid } }
    }
}
