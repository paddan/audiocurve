import Testing
@testable import AudioCurveCore

private func sel(_ ear: Ear, _ f: Double) -> PointSelection {
    PointSelection(ear: ear, frequencyHz: f)
}

private func threshold(_ p: HearingProfile, _ ear: Ear, _ f: Double) -> Double? {
    p.audiogram(ear).points.first { $0.frequencyHz == f }?.thresholdDBHL
}

@Test func snapCoarseAndFine() {
    #expect(AudiogramEditing.snap(83, fine: false) == 85)
    #expect(AudiogramEditing.snap(83, fine: true) == 83)
    #expect(AudiogramEditing.snap(130, fine: false) == 120)
    #expect(AudiogramEditing.snap(-14, fine: false) == -10)
}

@Test func setValueAddsMissingPoint() {
    var p = exampleProfile()
    p.left.points.removeAll { $0.frequencyHz == 750 }
    AudiogramEditing.setValue(&p, sel(.left, 750), dbHL: 40, fine: false)
    #expect(threshold(p, .left, 750) == 40)
    let freqs = p.left.points.map(\.frequencyHz)
    #expect(freqs == freqs.sorted())
    #expect(freqs.count == 11)
}

@Test func setValueIgnoresNonStandardFrequency() {
    var p = exampleProfile()
    let before = p
    AudiogramEditing.setValue(&p, sel(.left, 900), dbHL: 40, fine: false)
    #expect(p == before)
}

@Test func nudgeUpLowersHL() {
    var coarse = exampleProfile()
    AudiogramEditing.nudge(&coarse, sel(.left, 1500), up: true, fine: false)
    #expect(threshold(coarse, .left, 1500) == 60)
    var fine = exampleProfile()
    AudiogramEditing.nudge(&fine, sel(.left, 1500), up: true, fine: true)
    #expect(threshold(fine, .left, 1500) == 64)
}

@Test func nudgeDownRaisesHL() {
    var p = exampleProfile()
    AudiogramEditing.nudge(&p, sel(.left, 1500), up: false, fine: false)
    #expect(threshold(p, .left, 1500) == 70)
}

@Test func nudgeClamps() {
    var p = HearingProfile.flat()
    for i in p.left.points.indices { p.left.points[i].thresholdDBHL = -10 }
    AudiogramEditing.nudge(&p, sel(.left, 1000), up: true, fine: false)
    #expect(threshold(p, .left, 1000) == -10)
}

@Test func neighborSkipsMissing() {
    var p = HearingProfile.flat()
    p.left = audiogram(from: [500: 10, 1000: 10, 2000: 10])
    #expect(AudiogramEditing.neighbor(p, of: sel(.left, 500), forward: true) == sel(.left, 1000))
    #expect(AudiogramEditing.neighbor(p, of: sel(.left, 2000), forward: true) == nil)
    #expect(AudiogramEditing.neighbor(p, of: sel(.left, 2000), forward: false) == sel(.left, 1000))
    #expect(AudiogramEditing.neighbor(p, of: sel(.left, 500), forward: false) == nil)
}

@Test func switchEarSameOrNearest() {
    var p = HearingProfile.flat()
    p.left = audiogram(from: [1000: 10, 1500: 10])
    p.right = audiogram(from: [1000: 20, 2000: 20])
    #expect(AudiogramEditing.switchEar(p, sel(.left, 1000)) == sel(.right, 1000))
    #expect(AudiogramEditing.switchEar(p, sel(.left, 1500)) == sel(.right, 2000))
    #expect(AudiogramEditing.switchEar(p, sel(.right, 2000)) == sel(.left, 1500))
}

@Test func switchEarKeepsSelectionWhenOtherEarEmpty() {
    var p = HearingProfile.flat()
    p.right = Audiogram(points: [])
    #expect(AudiogramEditing.switchEar(p, sel(.left, 1000)) == sel(.left, 1000))
}

@Test func deleteRespectsMinimum() {
    var four = HearingProfile.flat()
    four.left = audiogram(from: [500: 10, 1000: 10, 2000: 10, 4000: 10])
    let before = four
    #expect(AudiogramEditing.delete(&four, sel(.left, 1000)) == false)
    #expect(four == before)

    var five = HearingProfile.flat()
    five.left = audiogram(from: [500: 10, 1000: 10, 2000: 10, 4000: 10, 6000: 10])
    #expect(AudiogramEditing.delete(&five, sel(.left, 1000)) == true)
    #expect(five.left.points.map(\.frequencyHz) == [500, 2000, 4000, 6000])
}

@Test func parseTyped() {
    #expect(AudiogramEditing.parseTyped("85") == 85)
    #expect(AudiogramEditing.parseTyped(" 85 ") == 85)
    #expect(AudiogramEditing.parseTyped("-5") == -5)
    #expect(AudiogramEditing.parseTyped("\u{2212}5") == -5)
    #expect(AudiogramEditing.parseTyped("130") == nil)
    #expect(AudiogramEditing.parseTyped("-11") == nil)
    #expect(AudiogramEditing.parseTyped("abc") == nil)
    #expect(AudiogramEditing.parseTyped("") == nil)
}

@Test func yMapping() {
    #expect(AudiogramEditing.dbHL(atY: 0, plotHeight: 130) == -10)
    #expect(AudiogramEditing.dbHL(atY: 130, plotHeight: 130) == 120)
    #expect(AudiogramEditing.dbHL(atY: 65, plotHeight: 130) == 55)
}

@Test func trimClampAndReset() {
    var p = HearingProfile.flat()
    AudiogramEditing.nudgeTrim(&p, ear: .left, band: 3, delta: 25)
    #expect(p.leftTrimDB[3] == 20)
    #expect(p.rightTrimDB[3] == 0)
    AudiogramEditing.nudgeTrim(&p, ear: .left, band: 3, delta: -45)
    #expect(p.leftTrimDB[3] == -20)
    AudiogramEditing.resetTrim(&p, ear: .left, band: 3)
    #expect(p.leftTrimDB[3] == 0)
}
