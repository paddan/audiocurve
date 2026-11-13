import Testing
import Foundation
@testable import AudioCurveCore

@Test func staleAndInactiveBeatsAreIgnored() {
    let now = Date(timeIntervalSince1970: 1000)
    let beats = [
        InstanceHeartbeat(pid: 1, hostBundleID: "com.rogueamoeba.soundsource", active: true, time: now.addingTimeInterval(-2)),
        InstanceHeartbeat(pid: 2, hostBundleID: "com.example.old", active: true, time: now.addingTimeInterval(-7)),
        InstanceHeartbeat(pid: 3, hostBundleID: "com.example.idle", active: false, time: now),
        InstanceHeartbeat(pid: 4, hostBundleID: "se.lindefors.audiocurve", active: true, time: now),
    ]
    #expect(InstanceHeartbeat.live(beats, now: now, excludingPID: 4).map(\.pid) == [1])
}

@Test func liveBeatsAreSortedByHost() {
    let now = Date(timeIntervalSince1970: 1000)
    let beats = [
        InstanceHeartbeat(pid: 1, hostBundleID: "com.z", active: true, time: now),
        InstanceHeartbeat(pid: 2, hostBundleID: "com.a", active: true, time: now),
    ]
    #expect(InstanceHeartbeat.live(beats, now: now, excludingPID: 9).map(\.hostBundleID) == ["com.a", "com.z"])
}

@Test func heartbeatsRoundTripThroughTheStore() throws {
    let dir = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
    defer { try? FileManager.default.removeItem(at: dir) }
    let store = ProfileStore(root: dir)
    let t = Date(timeIntervalSince1970: 1000)
    store.writeHeartbeat(InstanceHeartbeat(pid: 10, hostBundleID: "a", active: true, time: t))
    store.writeHeartbeat(InstanceHeartbeat(pid: 11, hostBundleID: "b", active: false, time: t))
    store.removeHeartbeat(pid: 10)
    try Data("garbage".utf8).write(to: dir.appendingPathComponent("instances/garbage.json"))
    #expect(store.readHeartbeats() == [InstanceHeartbeat(pid: 11, hostBundleID: "b", active: false, time: t)])
}

@Test func pruneRemovesLongDeadBeats() {
    let dir = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
    defer { try? FileManager.default.removeItem(at: dir) }
    let store = ProfileStore(root: dir)
    let now = Date(timeIntervalSince1970: 10_000)
    store.writeHeartbeat(InstanceHeartbeat(pid: 1, hostBundleID: "a", active: true, time: now.addingTimeInterval(-61)))
    store.writeHeartbeat(InstanceHeartbeat(pid: 2, hostBundleID: "b", active: true, time: now.addingTimeInterval(-10)))
    store.pruneHeartbeats(now: now)
    #expect(store.readHeartbeats().map(\.pid) == [2])
}
