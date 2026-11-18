import Foundation
import Testing
@testable import AudioCurveCore

private func scratchDefaults() -> UserDefaults {
    let name = "VolumeSnapshotTests.\(UUID().uuidString)"
    let d = UserDefaults(suiteName: name)!
    d.removePersistentDomain(forName: name)
    return d
}

@Test func volumeSnapshotRoundTripsThroughJSON() throws {
    let s = VolumeSnapshot(deviceUID: "dev-1", scalars: [0: 0.5, 1: 0.25, 2: 0.75])
    let back = try JSONDecoder().decode(VolumeSnapshot.self, from: JSONEncoder().encode(s))
    #expect(back == s)
}

@Test func pendingVolumeIsSavedReadAndCleared() {
    let d = scratchDefaults()
    #expect(VolumeSnapshot.pending(d) == [])
    let s = VolumeSnapshot(deviceUID: "dev-2", scalars: [1: 0.3, 2: 0.4])
    VolumeSnapshot.savePending(s, d)
    #expect(VolumeSnapshot.pending(d) == [s])
    VolumeSnapshot.clearPending(uid: "dev-2", d)
    #expect(VolumeSnapshot.pending(d) == [])
}

@Test func pendingVolumesOfTwoDevicesDoNotOverwriteEachOther() {
    let d = scratchDefaults()
    let a = VolumeSnapshot(deviceUID: "a", scalars: [0: 0.2])
    let b = VolumeSnapshot(deviceUID: "b", scalars: [0: 0.6])
    VolumeSnapshot.savePending(a, d)
    VolumeSnapshot.savePending(b, d)
    #expect(Set(VolumeSnapshot.pending(d).map(\.deviceUID)) == ["a", "b"])
    #expect(VolumeSnapshot.pending(d).first { $0.deviceUID == "a" } == a)
}

@Test func savingTheSameDeviceAgainKeepsTheOriginal() {
    let d = scratchDefaults()
    let original = VolumeSnapshot(deviceUID: "a", scalars: [0: 0.2])
    VolumeSnapshot.savePending(original, d)
    VolumeSnapshot.savePending(VolumeSnapshot(deviceUID: "a", scalars: [0: 0.5]), d)
    #expect(VolumeSnapshot.pending(d) == [original])
}

/// A restore that failed at pause leaves the original pending; raising again (volume now at max)
/// must not replace it with max.
@Test func pendingOriginalIsNeverReplacedByMax() {
    let d = scratchDefaults()
    let original = VolumeSnapshot(deviceUID: "a", scalars: [0: 0.3, 1: 0.3])
    VolumeSnapshot.savePending(original, d)
    VolumeSnapshot.savePending(VolumeSnapshot(deviceUID: "a", scalars: [0: 1, 1: 1]), d)
    #expect(VolumeSnapshot.pending(d).first { $0.deviceUID == "a" } == original)
    VolumeSnapshot.clearPending(uid: "a", d)
    let next = VolumeSnapshot(deviceUID: "a", scalars: [0: 0.6])
    VolumeSnapshot.savePending(next, d)
    #expect(VolumeSnapshot.pending(d) == [next])
}

/// After a partly failed restore only the failed elements stay pending; raising again adds the
/// others (now back at the user's level) without touching the pending originals.
@Test func savingAgainAddsOnlyMissingElements() {
    let d = scratchDefaults()
    VolumeSnapshot.savePending(VolumeSnapshot(deviceUID: "a", scalars: [1: 0.3], failedAttempts: 2), d)
    VolumeSnapshot.savePending(VolumeSnapshot(deviceUID: "a", scalars: [0: 0.4, 1: 1, 2: 1]), d)
    #expect(VolumeSnapshot.pending(uid: "a", d)
            == VolumeSnapshot(deviceUID: "a", scalars: [0: 0.4, 1: 0.3, 2: 1], failedAttempts: 2))
}

// MARK: Restore outcome

@Test func restoreIsDoneWhenEveryWritableElementWasRestored() {
    let s = VolumeSnapshot(deviceUID: "a", scalars: [0: 0.2, 1: 0.3, 2: 0.4])
    #expect(s.remaining(after: [0: .restored, 1: .restored, 2: .restored]) == nil)
    // An element that no longer accepts writes is dropped.
    #expect(s.remaining(after: [0: .restored, 1: .notWritable, 2: .notWritable]) == nil)
    #expect(s.remaining(after: [:]) == nil)
}

@Test func failedWritesAreKeptForALimitedNumberOfAttempts() {
    let s = VolumeSnapshot(deviceUID: "a", scalars: [0: 0.2, 1: 0.3])
    var left: VolumeSnapshot? = s
    var attempts = 0
    while let r = left {
        attempts += 1
        left = r.remaining(after: [0: .restored, 1: .failed])
        if let l = left {
            #expect(l.scalars == [1: 0.3])
            #expect(l.deviceUID == "a")
        }
        if attempts > 100 { break }
    }
    #expect(attempts == VolumeSnapshot.maxRestoreAttempts)
}

@Test func failedAttemptsSurviveSavingAndReading() throws {
    let s = VolumeSnapshot(deviceUID: "a", scalars: [1: 0.3])
    let after = try #require(s.remaining(after: [1: .failed]))
    let back = try JSONDecoder().decode(VolumeSnapshot.self, from: JSONEncoder().encode(after))
    #expect(back == after)
    #expect(back.failedAttempts == 1)
}

@Test func updatePendingReplacesWithWhatRemainsOrClears() {
    let d = scratchDefaults()
    let s = VolumeSnapshot(deviceUID: "a", scalars: [0: 0.2, 1: 0.3])
    let b = VolumeSnapshot(deviceUID: "b", scalars: [0: 0.6])
    VolumeSnapshot.savePending(s, d)
    VolumeSnapshot.savePending(b, d)
    let rest = s.remaining(after: [0: .restored, 1: .failed])
    VolumeSnapshot.updatePending(uid: "a", remaining: rest, d)
    #expect(VolumeSnapshot.pending(d).first { $0.deviceUID == "a" } == rest)
    VolumeSnapshot.updatePending(uid: "a", remaining: nil, d)
    #expect(VolumeSnapshot.pending(d) == [b])
}

@Test func snapshotWithoutAttemptsStillDecodes() throws {
    let json = Data(#"[{"deviceUID":"x","scalars":[0,0.5]}]"#.utf8)
    let d = scratchDefaults()
    d.set(json, forKey: VolumeSnapshot.pendingKey)
    #expect(VolumeSnapshot.pending(d) == [VolumeSnapshot(deviceUID: "x", scalars: [0: 0.5])])
}

@Test func clearingOneDeviceKeepsTheOther() {
    let d = scratchDefaults()
    let a = VolumeSnapshot(deviceUID: "a", scalars: [0: 0.2])
    let b = VolumeSnapshot(deviceUID: "b", scalars: [0: 0.6])
    VolumeSnapshot.savePending(a, d)
    VolumeSnapshot.savePending(b, d)
    VolumeSnapshot.clearPending(uid: "a", d)
    #expect(VolumeSnapshot.pending(d) == [b])
}

@Test func pendingVolumeUsesTheDocumentedKey() {
    let d = scratchDefaults()
    VolumeSnapshot.savePending(VolumeSnapshot(deviceUID: "x", scalars: [0: 1]), d)
    #expect(d.data(forKey: "hearingTest.pendingVolume") != nil)
}

@Test func oldSingleSlotValueIsStillRead() throws {
    let d = scratchDefaults()
    let old = VolumeSnapshot(deviceUID: "old", scalars: [0: 0.3])
    d.set(try JSONEncoder().encode(old), forKey: VolumeSnapshot.pendingKey)
    #expect(VolumeSnapshot.pending(d) == [old])
    let other = VolumeSnapshot(deviceUID: "new", scalars: [0: 0.7])
    VolumeSnapshot.savePending(other, d)
    #expect(Set(VolumeSnapshot.pending(d).map(\.deviceUID)) == ["old", "new"])
}

@Test func corruptPendingVolumeReadsAsEmpty() {
    let d = scratchDefaults()
    d.set(Data("nope".utf8), forKey: VolumeSnapshot.pendingKey)
    #expect(VolumeSnapshot.pending(d) == [])
}
