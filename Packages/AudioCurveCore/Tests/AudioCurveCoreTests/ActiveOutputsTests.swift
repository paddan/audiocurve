import Testing
import Foundation
@testable import AudioCurveCore

private let airpods = OutputDevice(uid: "airpods-uid", name: "AirPods Pro")
private let aids = OutputDevice(uid: "aids-uid", name: "Hearing aids")

@Test func emptySelectionIsActiveEverywhere() {
    let a = ActiveOutputs()
    #expect(a.isAll)
    #expect(a.isActive(currentUID: "anything"))
    #expect(a.isActive(currentUID: nil))
}

@Test func onlySelectedDevicesAreActive() {
    let a = ActiveOutputs(devices: [airpods])
    #expect(a.isActive(currentUID: "airpods-uid"))
    #expect(!a.isActive(currentUID: "aids-uid"))
    // An unknown current output never turns processing off.
    #expect(a.isActive(currentUID: nil))
}

@Test func toggleAddsAndRemoves() {
    var a = ActiveOutputs()
    a.toggle(airpods)
    a.toggle(aids)
    #expect(a.devices == [airpods, aids])
    a.toggle(airpods)
    #expect(a.devices == [aids])
}

@Test func menuListsAvailableThenRememberedOffline() {
    let a = ActiveOutputs(devices: [airpods])
    let speakers = OutputDevice(uid: "spk", name: "MacBook Pro Speakers")
    #expect(a.menuDevices(available: [speakers, aids]) == [speakers, aids, airpods])
    #expect(a.menuDevices(available: [airpods, speakers]) == [airpods, speakers])
}

@Test func activeOutputsRoundTripAndDefaults() throws {
    let dir = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
    defer { try? FileManager.default.removeItem(at: dir) }
    let store = ProfileStore(root: dir)
    #expect(store.loadActiveOutputs() == ActiveOutputs())
    store.saveActiveOutputs(ActiveOutputs(devices: [airpods]))
    #expect(store.loadActiveOutputs().devices == [airpods])
    try "not json".write(to: dir.appendingPathComponent("outputs.json"), atomically: true, encoding: .utf8)
    #expect(store.loadActiveOutputs() == ActiveOutputs())
}

@Test func paramsCarryActiveFlag() {
    let on = FittingRule.params(profile: nil, strength: 0.5, mpoDBFS: -3, outputGainDB: 0, mode: .music)
    #expect(on.active == 1)
    let off = FittingRule.params(profile: nil, strength: 0.5, mpoDBFS: -3, outputGainDB: 0, mode: .music, active: false)
    #expect(off.active == 0)
}
