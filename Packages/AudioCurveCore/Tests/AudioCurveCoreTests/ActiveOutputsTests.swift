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

private let speakers = OutputDevice(uid: "spk", name: "MacBook Pro Speakers")

@Test func everyBoxIsCheckedWhenAllAreActive() {
    let a = ActiveOutputs()
    #expect(a.isChecked("airpods-uid"))
    #expect(a.isChecked("spk"))
}

@Test func uncheckingFromAllKeepsTheOthers() {
    var a = ActiveOutputs()
    a.setEnabled(aids, false, available: [airpods, aids, speakers])
    #expect(a.devices == [airpods, speakers])
    #expect(!a.isChecked("aids-uid"))
}

@Test func checkingAndUncheckingSingleDevices() {
    var a = ActiveOutputs(devices: [airpods])
    a.setEnabled(speakers, true, available: [airpods, aids, speakers])
    #expect(a.devices == [airpods, speakers])
    a.setEnabled(speakers, false, available: [airpods, aids, speakers])
    #expect(a.devices == [airpods])
}

@Test func theLastCheckedBoxCannotBeUnchecked() {
    var a = ActiveOutputs(devices: [airpods])
    #expect(a.isLastChecked("airpods-uid", available: [airpods, aids]))
    a.setEnabled(airpods, false, available: [airpods, aids])
    #expect(a.devices == [airpods])
}

@Test func checkingEveryDeviceMeansAll() {
    var a = ActiveOutputs(devices: [airpods])
    a.setEnabled(aids, true, available: [airpods, aids])
    #expect(a.isAll)
}

@Test func menuListsAvailableThenRememberedOffline() {
    let a = ActiveOutputs(devices: [airpods])
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

@Test func ownRouteDevicesAreNotOutputs() {
    #expect(OutputDeviceMonitor.isOwnRoute(uid: "se.lindefors.audiocurve.route.1234"))
    #expect(!OutputDeviceMonitor.isOwnRoute(uid: "80-28-3C-24-CB-8C:output"))
}
