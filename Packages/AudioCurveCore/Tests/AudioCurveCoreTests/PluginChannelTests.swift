import Testing
import Foundation
@testable import AudioCurveCore

@Test func statusRoundTrips() {
    let s = PluginStatus(profiles: [ProfileSummary(id: UUID(), name: "Example")], activeProfileID: nil,
                         activeOutputs: ActiveOutputs(devices: [OutputDevice(uid: "a", name: "A")]),
                         otherHosts: ["com.example.host"])
    #expect(PluginChannel.status(from: [PluginChannel.statusKey: PluginChannel.encode(s), "other": 1]) == s)
}

@Test func commandsRoundTrip() {
    for c: PluginCommand in [.loadProfile(UUID()), .setActiveOutputs(ActiveOutputs()), .identifyHost("se.lindefors.audiocurve")] {
        #expect(PluginChannel.command(from: PluginChannel.commandState(c)) == c)
    }
}

@Test func unknownOrBadCommandIsNil() {
    #expect(PluginChannel.command(from: nil) == nil)
    #expect(PluginChannel.command(from: ["data": 1]) == nil)
    #expect(PluginChannel.command(from: [PluginChannel.commandKey: Data("{\"launchMissiles\":{}}".utf8)]) == nil)
    #expect(PluginChannel.status(from: [PluginChannel.statusKey: "nope"]) == nil)
    #expect(PluginChannel.status(from: nil) == nil)
}
