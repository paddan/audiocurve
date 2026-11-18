import Foundation
import Testing
@testable import AudioCurveCore

private let uidA = UUID()
private let uidB = UUID()
private let lv = LevelSettings(strength: 0.8, inputGainDB: 1, outputGainDB: -2, tone: 0.1, mpoDBFS: -6)
private func edited() -> HearingProfile { exampleProfile() }
private func assignment(_ profile: UUID?, _ levels: LevelSettings? = nil) -> OutputAssignment {
    OutputAssignment(device: OutputDevice(uid: "u1", name: "Dev"), profileID: profile, levels: levels)
}

@Test func noAssignmentDoesNothing() {
    let wc = WorkingCopy(profile: edited(), sourceID: uidA)
    #expect(OutputSwitch.actions(assignment: nil, workingCopy: wc, savedSource: .flat(), existing: [uidA, uidB]) == [])
}

@Test func noWorkingCopyJustLoads() {
    #expect(OutputSwitch.actions(assignment: assignment(uidB), workingCopy: nil, savedSource: nil, existing: [uidB]) == [.load(uidB)])
}

@Test func dirtySourceIsOverwrittenFirst() {
    let wc = WorkingCopy(profile: edited(), sourceID: uidA)
    #expect(OutputSwitch.actions(assignment: assignment(uidB), workingCopy: wc, savedSource: .flat(), existing: [uidA, uidB])
            == [.overwrite(uidA, edited()), .load(uidB)])
}

@Test func cleanSourceIsNotOverwritten() {
    let wc = WorkingCopy(profile: edited(), sourceID: uidA)
    #expect(OutputSwitch.actions(assignment: assignment(uidB), workingCopy: wc, savedSource: edited(), existing: [uidA, uidB]) == [.load(uidB)])
}

@Test func unsavedNonFlatIsSaved() {
    let wc = WorkingCopy(profile: edited(), sourceID: nil)
    #expect(OutputSwitch.actions(assignment: assignment(uidB), workingCopy: wc, savedSource: nil, existing: [uidB])
            == [.saveUnsaved(edited()), .load(uidB)])
}

@Test func flatUnsavedIsNotSaved() {
    let wc = WorkingCopy(profile: .flat(), sourceID: nil)
    #expect(OutputSwitch.actions(assignment: assignment(uidB), workingCopy: wc, savedSource: nil, existing: [uidB]) == [.load(uidB)])
}

@Test func sameProfileKeepsEdits() {
    let wc = WorkingCopy(profile: edited(), sourceID: uidA)
    #expect(OutputSwitch.actions(assignment: assignment(uidA), workingCopy: wc, savedSource: .flat(), existing: [uidA]) == [])
    #expect(OutputSwitch.actions(assignment: assignment(uidA, lv), workingCopy: wc, savedSource: .flat(), existing: [uidA]) == [.applyLevels(lv)])
}

@Test func deletedAssignedProfileIgnored() {
    let wc = WorkingCopy(profile: edited(), sourceID: uidA)
    #expect(OutputSwitch.actions(assignment: assignment(uidB, lv), workingCopy: wc, savedSource: .flat(), existing: [uidA]) == [.applyLevels(lv)])
    #expect(OutputSwitch.actions(assignment: assignment(uidB), workingCopy: wc, savedSource: .flat(), existing: [uidA]) == [])
}

/// The new output's levels (and ceiling) come before its profile, so a stronger profile never
/// runs with the old output's levels.
@Test func levelsAppliedBeforeLoad() {
    #expect(OutputSwitch.actions(assignment: assignment(uidB, lv), workingCopy: nil, savedSource: nil, existing: [uidB]) == [.applyLevels(lv), .load(uidB)])
    let wc = WorkingCopy(profile: edited(), sourceID: uidA)
    #expect(OutputSwitch.actions(assignment: assignment(uidB, lv), workingCopy: wc, savedSource: .flat(), existing: [uidA, uidB])
            == [.overwrite(uidA, edited()), .applyLevels(lv), .load(uidB)])
}

@Test func pruneClearsDeleted() {
    var a = OutputAssignments()
    a.assign(OutputDevice(uid: "x", name: "X"), profile: uidB, levels: lv)
    a.assign(OutputDevice(uid: "y", name: "Y"), profile: uidB, levels: nil)
    a.prune(existing: [])
    #expect(a.assignment("x")?.profileID == nil)
    #expect(a.assignment("x")?.levels == lv)
    #expect(a.assignment("y") == nil)
}

@Test func setLevelsKeepsProfile() {
    var a = OutputAssignments()
    a.assign(OutputDevice(uid: "x", name: "X"), profile: uidB, levels: nil)
    a.setLevels(lv, for: "x")
    #expect(a.assignment("x")?.profileID == uidB)
    #expect(a.assignment("x")?.levels == lv)
    a.setLevels(lv, for: "unknown")
    #expect(a.assignment("unknown") == nil)
}
