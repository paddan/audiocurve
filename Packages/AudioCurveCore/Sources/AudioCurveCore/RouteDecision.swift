/// Whether the user has allowed AudioCurve to record system audio. There is no public API to
/// read it, so `.unknown` means "not seen denied yet": the first tap triggers the system prompt.
public enum PermissionState: Sendable, Equatable { case granted, denied, unknown }

public enum PluginLoadState: Sendable, Equatable { case loading, loaded, failed }

public enum RouteFailure: Sendable, Equatable { case permission, plugin, route }

/// What the system-wide route should be. Only `.running` has a tap; in every other state the
/// Mac plays sound normally.
public enum RouteTarget: Sendable, Equatable {
    case off
    case starting
    case paused(deviceUID: String)
    case running(deviceUID: String)
    case failed(RouteFailure)
}

public struct RouteInputs: Sendable, Equatable {
    public var enabled: Bool
    public var permission: PermissionState
    public var plugin: PluginLoadState
    /// The system's default output device, nil if it cannot be read.
    public var outputUID: String?
    public var activeOutputs: ActiveOutputs

    public init(enabled: Bool, permission: PermissionState, plugin: PluginLoadState,
                outputUID: String?, activeOutputs: ActiveOutputs) {
        self.enabled = enabled
        self.permission = permission
        self.plugin = plugin
        self.outputUID = outputUID
        self.activeOutputs = activeOutputs
    }
}

/// The target route for `inputs`. First match wins: off, denied permission, plugin failed,
/// plugin loading, unknown output, output not ticked in "Active on", else running.
public func routeTarget(for inputs: RouteInputs) -> RouteTarget {
    guard inputs.enabled else { return .off }
    if inputs.permission == .denied { return .failed(.permission) }
    switch inputs.plugin {
    case .failed: return .failed(.plugin)
    case .loading: return .starting
    case .loaded: break
    }
    guard let uid = inputs.outputUID else { return .failed(.route) }
    return inputs.activeOutputs.isActive(currentUID: uid) ? .running(deviceUID: uid) : .paused(deviceUID: uid)
}
