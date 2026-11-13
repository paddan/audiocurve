import Testing
import Foundation
@testable import AudioCurveCore

private struct BuildError: Error {}

@MainActor
private final class FakeBuilder: RouteBuilding {
    var log: [String] = []
    var failuresLeft = 0
    func build(deviceUID: String) throws -> BuiltRoute {
        log.append("build:\(deviceUID)")
        if failuresLeft > 0 { failuresLeft -= 1; throw BuildError() }
        return BuiltRoute(stopIO: { self.log.append("stopIO") },
                          destroyAggregate: { self.log.append("destroyAggregate") },
                          destroyTap: { self.log.append("destroyTap") })
    }
}

@MainActor
private final class Harness {
    let builder = FakeBuilder()
    var queue: [@MainActor () -> Void] = []
    var delays: [TimeInterval] = []
    var clock = Date(timeIntervalSince1970: 1000)
    var reloads = 0
    lazy var controller: RouteController = {
        let c = RouteController(builder: builder, now: { [unowned self] in self.clock },
                                after: { [unowned self] d, f in self.delays.append(d); self.queue.append(f) })
        c.onReloadPlugin = { [unowned self] in self.reloads += 1 }
        return c
    }()
    /// Runs the closures scheduled so far (not ones they schedule).
    func fire() { let q = queue; queue = []; q.forEach { $0() } }
    var log: [String] { builder.log }
    static func inputs(output: String? = "airpods", active: ActiveOutputs = ActiveOutputs()) -> RouteInputs {
        RouteInputs(enabled: true, permission: .granted, plugin: .loaded, outputUID: output, activeOutputs: active)
    }
    /// A controller already running on AirPods, with the log cleared.
    static func running() -> Harness {
        let h = Harness()
        h.controller.inputs = inputs()
        h.fire()
        h.builder.log = []
        return h
    }
}

private let teardown = ["stopIO", "destroyAggregate", "destroyTap"]

@MainActor @Test func runningBuildsOnceAfterCoalescing() {
    let h = Harness()
    h.controller.inputs = Harness.inputs()
    #expect(h.log.isEmpty)
    #expect(h.delays == [RouteController.coalesce])
    h.fire()
    #expect(h.log == ["build:airpods"])
    #expect(h.controller.state == .running(deviceUID: "airpods"))
}

@MainActor @Test func burstOfChangesCoalescesIntoOneBuild() {
    let h = Harness()
    h.controller.inputs = Harness.inputs(output: "a")
    h.controller.inputs = Harness.inputs(output: "b")
    h.controller.inputs = Harness.inputs(output: "airpods")
    h.fire()
    #expect(h.log == ["build:airpods"])
}

@MainActor @Test func deviceLossTearsDownBeforeRebuild() {
    let h = Harness.running()
    h.controller.inputs = Harness.inputs(output: "spk")
    h.fire()
    #expect(h.log == teardown + ["build:spk"])
    #expect(h.controller.state == .running(deviceUID: "spk"))
}

@MainActor @Test func pausingTearsDownAndDoesNotBuild() {
    let h = Harness.running()
    let onlyAids = ActiveOutputs(devices: [OutputDevice(uid: "aids", name: "Hearing aids")])
    h.controller.inputs = Harness.inputs(active: onlyAids)
    h.fire()
    #expect(h.log == teardown)
    #expect(h.controller.state == .paused(deviceUID: "airpods"))
}

@MainActor @Test func buildFailureRetriesOnceThenFails() {
    let h = Harness()
    h.builder.failuresLeft = 2
    h.controller.inputs = Harness.inputs()
    h.fire()
    #expect(h.delays.last == RouteController.retryDelay)
    h.fire()
    #expect(h.log == ["build:airpods", "build:airpods"])
    #expect(h.controller.state == .failed(.route))
    #expect(h.queue.isEmpty)
}

@MainActor @Test func retrySucceeds() {
    let h = Harness()
    h.builder.failuresLeft = 1
    h.controller.inputs = Harness.inputs()
    h.fire(); h.fire()
    #expect(h.controller.state == .running(deviceUID: "airpods"))
}

@MainActor @Test func pluginDeathTearsDownImmediatelyAndReloads() {
    let h = Harness.running()
    h.controller.pluginDied()
    #expect(h.log == teardown)
    #expect(h.reloads == 1)
    #expect(h.controller.inputs.plugin == .loading)
    #expect(h.controller.state == .starting)
}

@MainActor @Test func secondPluginDeathWithinTenSecondsFails() {
    let h = Harness.running()
    h.controller.pluginDied()
    h.controller.inputs.plugin = .loaded
    h.fire()
    h.clock = h.clock.addingTimeInterval(5)
    h.controller.pluginDied()
    #expect(h.controller.state == .failed(.plugin))
    #expect(h.reloads == 1)
    #expect(h.log == teardown + ["build:airpods"] + teardown)
}

@MainActor @Test func pluginDeathAfterTenSecondsReloadsAgain() {
    let h = Harness.running()
    h.controller.pluginDied()
    h.controller.inputs.plugin = .loaded
    h.fire()
    h.clock = h.clock.addingTimeInterval(11)
    h.controller.pluginDied()
    #expect(h.reloads == 2)
}

@MainActor @Test func unchangedTargetDoesNotRebuild() {
    let h = Harness.running()
    h.controller.inputs = Harness.inputs()
    h.fire()
    #expect(h.log.isEmpty)
}

@MainActor @Test func invalidateRebuildsSameDevice() {
    let h = Harness.running()
    h.controller.invalidate()
    h.fire()
    #expect(h.log == teardown + ["build:airpods"])
}

@MainActor @Test func shutDownTearsDown() {
    let h = Harness.running()
    h.controller.shutDown()
    #expect(h.log == teardown)
    #expect(h.controller.state == .off)
    h.controller.inputs = Harness.inputs(output: "spk")
    h.fire()
    #expect(h.log == teardown)
}

@MainActor @Test func stateChangesAreReportedOnce() {
    let h = Harness()
    var seen: [RouteTarget] = []
    h.controller.onStateChange = { seen.append($0) }
    h.controller.inputs = Harness.inputs()
    h.fire()
    h.controller.inputs = Harness.inputs()
    h.fire()
    #expect(seen == [.running(deviceUID: "airpods")])
}

@MainActor @Test func stateIsNotRunningWhileARetryIsPending() {
    let h = Harness.running()
    h.builder.failuresLeft = 1
    h.controller.inputs = Harness.inputs(output: "spk")
    h.fire()
    #expect(h.controller.state == .starting)
    h.fire()
    #expect(h.controller.state == .running(deviceUID: "spk"))
}
