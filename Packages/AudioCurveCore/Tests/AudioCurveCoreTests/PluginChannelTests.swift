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

@Test func newCommandsRoundTrip() {
    let dev = OutputDevice(uid: "airpods", name: "AirPods Pro")
    let record = HearingTestRecord(date: Date(timeIntervalSince1970: 1_700_000_000), output: dev,
                                   calibration: .nominal(.inEar),
                                   left: [TestPoint(frequencyHz: 1000, dBFS: -40, hl: 20)], right: [],
                                   flags: [], appVersion: "2.2.0")
    let commands: [PluginCommand] = [
        .assignProfile(dev, UUID()),
        .assignProfile(dev, nil),
        .saveMeasuredProfile(id: UUID(), name: "Measured", profile: .flat(), measurement: record, assignTo: dev),
        .saveMeasuredProfile(id: UUID(), name: "Measured", profile: .flat(), measurement: record, assignTo: nil),
    ]
    for c in commands {
        #expect(PluginChannel.command(from: PluginChannel.commandState(c)) == c)
    }
}

@Test func statusCarriesAssignments() {
    let dev = OutputDevice(uid: "airpods", name: "AirPods Pro")
    var assignments = OutputAssignments()
    assignments.assign(dev, profile: UUID(), levels: nil)
    let s = PluginStatus(profiles: [], activeProfileID: nil, activeOutputs: ActiveOutputs(),
                         otherHosts: [], assignments: assignments)
    #expect(PluginChannel.status(from: [PluginChannel.statusKey: PluginChannel.encode(s)]) == s)
}
