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
    let saved = try store.saveNew(name: "  Hemma  ", profile: userProfile())
    #expect(saved.name == "Hemma")
    let list = store.list()
    #expect(list.count == 1)
    #expect(list[0].id == saved.id)
    #expect(list[0].profile == userProfile())
    #expect(store.saved(id: saved.id)?.name == "Hemma")
    #expect(store.saved(id: UUID()) == nil)
}

@Test func listIsSortedByName() throws {
    let (store, dir) = tempStore(); defer { try? FileManager.default.removeItem(at: dir) }
    for n in ["Zeta", "alfa", "Beta 10", "Beta 2"] { try store.saveNew(name: n, profile: .flat()) }
    #expect(store.list().map(\.name) == ["alfa", "Beta 2", "Beta 10", "Zeta"])
}

@Test func duplicateAndEmptyNamesRejected() throws {
    let (store, dir) = tempStore(); defer { try? FileManager.default.removeItem(at: dir) }
    try store.saveNew(name: "Hemma", profile: .flat())
    #expect(throws: ProfileStoreError.nameTaken) { try store.saveNew(name: " hemma ", profile: .flat()) }
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
    try store.overwrite(id: saved.id, profile: userProfile())
    let again = try #require(store.saved(id: saved.id))
    #expect(again.profile == userProfile())
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
    let x = try store.saveNew(name: "X", profile: userProfile())
    let c1 = try store.duplicate(id: x.id)
    let c2 = try store.duplicate(id: x.id)
    #expect(c1.name == "X kopia")
    #expect(c2.name == "X kopia 2")
    #expect(c1.id != x.id)
    #expect(c1.profile == userProfile())
    #expect(store.list().count == 3)
    #expect(throws: ProfileStoreError.notFound) { try store.duplicate(id: UUID()) }
}

@Test func duplicateUsesInjectedCopyName() throws {
    let (store, dir) = tempStore(); defer { try? FileManager.default.removeItem(at: dir) }
    let x = try store.saveNew(name: "X", profile: .flat())
    let name: (String, Int) -> String = { $1 == 1 ? "\($0) copy" : "\($0) copy \($1)" }
    #expect(try store.duplicate(id: x.id, copyName: name).name == "X copy")
    #expect(try store.duplicate(id: x.id, copyName: name).name == "X copy 2")
    #expect(try store.duplicate(id: x.id).name == "X kopia")
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
    store.saveWorkingCopy(WorkingCopy(profile: userProfile(), sourceID: id))
    #expect(store.loadWorkingCopy() == WorkingCopy(profile: userProfile(), sourceID: id))
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
