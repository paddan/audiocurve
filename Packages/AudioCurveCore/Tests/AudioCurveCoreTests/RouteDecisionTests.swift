import Testing
@testable import AudioCurveCore

private func inputs(enabled: Bool = true, permission: PermissionState = .granted,
                    plugin: PluginLoadState = .loaded, output: String? = "airpods",
                    active: ActiveOutputs = ActiveOutputs()) -> RouteInputs {
    RouteInputs(enabled: enabled, permission: permission, plugin: plugin, outputUID: output, activeOutputs: active)
}

@Test func offWinsOverEverything() {
    #expect(routeTarget(for: inputs(enabled: false, permission: .denied, plugin: .failed)) == .off)
}

@Test func deniedPermissionFails() {
    #expect(routeTarget(for: inputs(permission: .denied)) == .failed(.permission))
}

@Test func unknownPermissionStillRuns() {
    #expect(routeTarget(for: inputs(permission: .unknown)) == .running(deviceUID: "airpods"))
}

@Test func pluginStates() {
    #expect(routeTarget(for: inputs(plugin: .loading)) == .starting)
    #expect(routeTarget(for: inputs(plugin: .failed)) == .failed(.plugin))
}

@Test func unknownOutputFailsRoute() {
    #expect(routeTarget(for: inputs(output: nil)) == .failed(.route))
}

@Test func untickedOutputPauses() {
    let a = ActiveOutputs(devices: [OutputDevice(uid: "airpods", name: "AirPods Pro")])
    #expect(routeTarget(for: inputs(output: "aids", active: a)) == .paused(deviceUID: "aids"))
    #expect(routeTarget(for: inputs(output: "airpods", active: a)) == .running(deviceUID: "airpods"))
}

@Test func emptySelectionRunsEverywhere() {
    #expect(routeTarget(for: inputs(output: "spk")) == .running(deviceUID: "spk"))
}
