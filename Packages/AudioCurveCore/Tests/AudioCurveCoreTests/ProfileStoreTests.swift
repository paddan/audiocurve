import Foundation
import Testing
@testable import AudioCurveCore

private func tempStore() -> (ProfileStore, URL) {
    let dir = FileManager.default.temporaryDirectory.appendingPathComponent("audiocurve-test-\(UUID().uuidString)")
    return (ProfileStore(root: dir), dir)
}

private func profileFile(_ dir: URL, _ id: UUID) -> URL {
    dir.appendingPathComponent("profiles").appendingPathComponent("\(id.uuidString).json")
}

@Test func saveNewAndListRoundTrip() throws {
    let (store, dir) = tempStore(); defer { try? FileManager.default.removeItem(at: dir) }
    let saved = try store.saveNew(name: "  Home  ", profile: exampleProfile())
    #expect(saved.name == "Home")
    let list = store.list()
    #expect(list.count == 1)
    #expect(list[0].id == saved.id)
    #expect(list[0].profile == exampleProfile())
    #expect(store.saved(id: saved.id)?.name == "Home")
    #expect(store.saved(id: UUID()) == nil)
}

@Test func listIsSortedByName() throws {
    let (store, dir) = tempStore(); defer { try? FileManager.default.removeItem(at: dir) }
    for n in ["Zeta", "alfa", "Beta 10", "Beta 2"] { try store.saveNew(name: n, profile: .flat()) }
    #expect(store.list().map(\.name) == ["alfa", "Beta 2", "Beta 10", "Zeta"])
}

@Test func duplicateAndEmptyNamesRejected() throws {
    let (store, dir) = tempStore(); defer { try? FileManager.default.removeItem(at: dir) }
    try store.saveNew(name: "Home", profile: .flat())
    #expect(throws: ProfileStoreError.nameTaken) { try store.saveNew(name: " home ", profile: .flat()) }
    #expect(throws: ProfileStoreError.emptyName) { try store.saveNew(name: "   ", profile: .flat()) }
    #expect(store.list().count == 1)
}

@Test func longNamesAreTruncated() throws {
    let (store, dir) = tempStore(); defer { try? FileManager.default.removeItem(at: dir) }
    let saved = try store.saveNew(name: String(repeating: "a", count: 100), profile: .flat())
    #expect(saved.name.count == 60)
}

@Test func invalidProfileRejected() throws {
    let (store, dir) = tempStore(); defer { try? FileManager.default.removeItem(at: dir) }
    var bad = HearingProfile.flat()
    bad.leftTrimDB = []
    #expect(throws: ProfileStoreError.io("invalid profile")) { try store.saveNew(name: "X", profile: bad) }
    let ok = try store.saveNew(name: "X", profile: .flat())
    #expect(throws: ProfileStoreError.io("invalid profile")) { try store.overwrite(id: ok.id, profile: bad) }
}

@Test func overwriteReplacesProfileAndUpdatesModified() throws {
    let (store, dir) = tempStore(); defer { try? FileManager.default.removeItem(at: dir) }
    let saved = try store.saveNew(name: "X", profile: .flat())
    try store.overwrite(id: saved.id, profile: exampleProfile())
    let again = try #require(store.saved(id: saved.id))
    #expect(again.profile == exampleProfile())
    #expect(again.modified >= saved.modified)
    #expect(again.name == "X")
    #expect(throws: ProfileStoreError.notFound) { try store.overwrite(id: UUID(), profile: .flat()) }
}

@Test func renameIncludingToOwnName() throws {
    let (store, dir) = tempStore(); defer { try? FileManager.default.removeItem(at: dir) }
    let a = try store.saveNew(name: "A", profile: .flat())
    try store.saveNew(name: "B", profile: .flat())
    try store.rename(id: a.id, to: "a")                 // own name, different case
    #expect(store.saved(id: a.id)?.name == "a")
    try store.rename(id: a.id, to: "C")
    #expect(store.saved(id: a.id)?.name == "C")
    #expect(throws: ProfileStoreError.nameTaken) { try store.rename(id: a.id, to: "b") }
    #expect(throws: ProfileStoreError.emptyName) { try store.rename(id: a.id, to: " ") }
    #expect(throws: ProfileStoreError.notFound) { try store.rename(id: UUID(), to: "Q") }
}

@Test func duplicateNaming() throws {
    let (store, dir) = tempStore(); defer { try? FileManager.default.removeItem(at: dir) }
    let x = try store.saveNew(name: "X", profile: exampleProfile())
    let c1 = try store.duplicate(id: x.id)
    let c2 = try store.duplicate(id: x.id)
    #expect(c1.name == "X copy")
    #expect(c2.name == "X copy 2")
    #expect(c1.id != x.id)
    #expect(c1.profile == exampleProfile())
    #expect(store.list().count == 3)
    #expect(throws: ProfileStoreError.notFound) { try store.duplicate(id: UUID()) }
}

@Test func duplicateUsesInjectedCopyName() throws {
    let (store, dir) = tempStore(); defer { try? FileManager.default.removeItem(at: dir) }
    let x = try store.saveNew(name: "X", profile: .flat())
    let name: (String, Int) -> String = { $1 == 1 ? "\($0) Kopie" : "\($0) Kopie \($1)" }
    #expect(try store.duplicate(id: x.id, copyName: name).name == "X Kopie")
    #expect(try store.duplicate(id: x.id, copyName: name).name == "X Kopie 2")
    #expect(try store.duplicate(id: x.id).name == "X copy")
}

@Test func deleteRemoves() throws {
    let (store, dir) = tempStore(); defer { try? FileManager.default.removeItem(at: dir) }
    let x = try store.saveNew(name: "X", profile: .flat())
    try store.delete(id: x.id)
    #expect(store.list().isEmpty)
    #expect(throws: ProfileStoreError.notFound) { try store.delete(id: x.id) }
    #expect(throws: ProfileStoreError.notFound) { try store.delete(id: UUID()) }
}

@Test func corruptAndWrongVersionFilesAreSkipped() throws {
    let (store, dir) = tempStore(); defer { try? FileManager.default.removeItem(at: dir) }
    let good = try store.saveNew(name: "Bra", profile: .flat())
    let corrupt = try store.saveNew(name: "Trasig", profile: .flat())
    let future = try store.saveNew(name: "Framtid", profile: .flat())
    try Data("{".utf8).write(to: profileFile(dir, corrupt.id))
    let url = profileFile(dir, future.id)
    let text = try String(contentsOf: url, encoding: .utf8)
    #expect(text.contains("\"version\""))
    let data = try Data(contentsOf: url)
    var obj = try #require(JSONSerialization.jsonObject(with: data) as? [String: Any])
    obj["version"] = 2
    try JSONSerialization.data(withJSONObject: obj).write(to: url)
    #expect(store.list().map(\.id) == [good.id])
    #expect(store.saved(id: corrupt.id) == nil)
    #expect(store.saved(id: future.id) == nil)
}

@Test func workingCopyRoundTrip() throws {
    let (store, dir) = tempStore(); defer { try? FileManager.default.removeItem(at: dir) }
    let id = UUID()
    store.saveWorkingCopy(WorkingCopy(profile: exampleProfile(), sourceID: id))
    #expect(store.loadWorkingCopy() == WorkingCopy(profile: exampleProfile(), sourceID: id))
    store.saveWorkingCopy(WorkingCopy(profile: .flat(), sourceID: nil))
    #expect(store.loadWorkingCopy() == WorkingCopy(profile: .flat(), sourceID: nil))
}

@Test func workingCopyMissingOrCorruptIsNil() throws {
    let (store, dir) = tempStore(); defer { try? FileManager.default.removeItem(at: dir) }
    #expect(store.loadWorkingCopy() == nil)
    store.saveWorkingCopy(WorkingCopy(profile: .flat(), sourceID: nil))
    try Data("nope".utf8).write(to: dir.appendingPathComponent("current.json"))
    #expect(store.loadWorkingCopy() == nil)
}

// MARK: Assignments, measurement, unique names

private func sampleRecord() -> HearingTestRecord {
    HearingTestRecord(date: Date(timeIntervalSince1970: 1_700_000_000),
                      output: OutputDevice(uid: "u1", name: "AirPods Pro"),
                      calibration: TestCalibration(transducer: .inEar),
                      left: [TestPoint(frequencyHz: 1000, dBFS: -50, hl: 30)],
                      right: [TestPoint(frequencyHz: 1000, dBFS: -55, hl: 25)],
                      flags: [.asymmetry], appVersion: "2.2.0")
}

@Test func assignmentsRoundTrip() throws {
    let (store, dir) = tempStore(); defer { try? FileManager.default.removeItem(at: dir) }
    #expect(store.loadAssignments() == OutputAssignments())
    var a = OutputAssignments()
    a.assign(OutputDevice(uid: "x", name: "X"), profile: UUID(), levels: LevelSettings(strength: 1, inputGainDB: 0, outputGainDB: 0, tone: 0, mpoDBFS: -3))
    store.saveAssignments(a)
    #expect(store.loadAssignments() == a)
}

@Test func measurementRoundTripsAndOldFilesDecode() throws {
    let (store, dir) = tempStore(); defer { try? FileManager.default.removeItem(at: dir) }
    let id = UUID()
    try store.saveNew(id: id, name: "Tested", profile: exampleProfile(), measurement: sampleRecord(), uniqueName: false)
    #expect(store.saved(id: id)?.measurement == sampleRecord())
    let old = try store.saveNew(name: "Old", profile: exampleProfile())
    let file = profileFile(dir, old.id)
    let text = try String(contentsOf: file, encoding: .utf8)
    #expect(!text.contains("measurement"))
    #expect(store.saved(id: old.id)?.measurement == nil)
}

@Test func uniqueNameAppendsSuffix() throws {
    let (store, dir) = tempStore(); defer { try? FileManager.default.removeItem(at: dir) }
    let n = "AirPods Pro – 16 Nov 2026"
    let a = try store.saveNew(id: UUID(), name: n, profile: .flat(), measurement: nil, uniqueName: true)
    let b = try store.saveNew(id: UUID(), name: n.uppercased(), profile: .flat(), measurement: nil, uniqueName: true)
    let c = try store.saveNew(id: UUID(), name: n, profile: .flat(), measurement: nil, uniqueName: true)
    #expect(a.name == n)
    #expect(b.name == n.uppercased() + " (2)")
    #expect(c.name == n + " (3)")
}

@Test func uniqueNameTruncatesBeforeSuffix() throws {
    let (store, dir) = tempStore(); defer { try? FileManager.default.removeItem(at: dir) }
    let base = String(repeating: "a", count: 55) + " " + String(repeating: "b", count: 14)
    try store.saveNew(id: UUID(), name: base, profile: .flat(), measurement: nil, uniqueName: true)
    let second = try store.saveNew(id: UUID(), name: base, profile: .flat(), measurement: nil, uniqueName: true)
    #expect(second.name.count <= ProfileStore.maxNameLength)
    #expect(second.name.hasSuffix(" (2)"))
    #expect(!second.name.contains("  "))
    let spaced = String(repeating: "c", count: 56) + " " + "dddd"
    try store.saveNew(id: UUID(), name: spaced, profile: .flat(), measurement: nil, uniqueName: true)
    let s2 = try store.saveNew(id: UUID(), name: spaced, profile: .flat(), measurement: nil, uniqueName: true)
    #expect(s2.name == String(repeating: "c", count: 56) + " (2)")
}

@Test func saveNewUsesGivenID() throws {
    let (store, dir) = tempStore(); defer { try? FileManager.default.removeItem(at: dir) }
    let id = UUID()
    let s = try store.saveNew(id: id, name: "X", profile: .flat(), measurement: nil, uniqueName: false)
    #expect(s.id == id)
    #expect(store.saved(id: id) != nil)
}
