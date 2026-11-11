import Testing
@testable import AudioCurveCore

@Test func flatIsValid() {
    let p = HearingProfile.flat()
    #expect(validationError(p) == nil)
    #expect(p.left.points.count == 11 && p.right.points.count == 11)
}

@Test func tooFewPoints() {
    var p = HearingProfile.flat()
    p.left = Audiogram(points: p.left.points.prefix(3).map { $0 })
    #expect(validationError(p) == .tooFewPoints(.left))
}

@Test func unsortedOrDuplicate() {
    var p = HearingProfile.flat()
    p.left = Audiogram(points: [1000, 500, 2000, 4000].map { AudiogramPoint(frequencyHz: $0, thresholdDBHL: 0) })
    #expect(validationError(p) == .unsortedOrDuplicateFrequencies(.left))
}

@Test func nonStandardFrequency() {
    var p = HearingProfile.flat()
    p.left = Audiogram(points: [500, 900, 1000, 2000].map { AudiogramPoint(frequencyHz: $0, thresholdDBHL: 0) })
    #expect(validationError(p) == .nonStandardFrequency(.left, 900))
}

@Test func thresholdBounds() {
    func profile(_ hl: Double) -> HearingProfile {
        var p = HearingProfile.flat()
        p.left = Audiogram(points: [125, 250, 500, 1000].map { AudiogramPoint(frequencyHz: $0, thresholdDBHL: hl) })
        return p
    }
    #expect(validationError(profile(120)) == nil)
    #expect(validationError(profile(120.5)) != nil)
    #expect(validationError(profile(-10)) == nil)
    #expect(validationError(profile(-10.5)) != nil)
}

@Test func trimBounds() {
    var p = HearingProfile.flat()
    p.leftTrimDB[3] = 20
    #expect(validationError(p) == nil)
    p.leftTrimDB[3] = 20.5
    #expect(validationError(p) == .trimOutOfRange(.left, band: 3))
    var q = HearingProfile.flat()
    q.leftTrimDB = Array(repeating: 0, count: 11)
    #expect(validationError(q) == .wrongTrimCount(.left))
}

@Test func audiogramAccessors() {
    var p = HearingProfile.flat()
    let a = Audiogram(points: [125, 250, 500, 1000].map { AudiogramPoint(frequencyHz: $0, thresholdDBHL: 30) })
    p.setAudiogram(a, for: .right)
    p.setTrim([1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12], for: .right)
    #expect(p.audiogram(.right) == a)
    #expect(p.trim(.right) == [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12])
    #expect(p.trim(.left) == Array(repeating: 0, count: 12))
}
