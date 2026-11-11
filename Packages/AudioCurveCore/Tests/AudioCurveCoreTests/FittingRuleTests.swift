import Testing
import CAudioCurveDSP
@testable import AudioCurveCore

private func flatAudiogram(_ hl: Double) -> Audiogram {
    Audiogram(points: Audiogram.standardFrequencies.map { AudiogramPoint(frequencyHz: $0, thresholdDBHL: hl) })
}

private func uniformProfile(_ hl: Double) -> HearingProfile {
    HearingProfile(left: flatAudiogram(hl), right: flatAudiogram(hl),
                   leftTrimDB: Array(repeating: 0, count: 12), rightTrimDB: Array(repeating: 0, count: 12))
}

private func expectFitting(_ b: BandFitting, _ soft: Double, _ loud: Double, _ cr: Double,
                           sourceLocation: SourceLocation = #_sourceLocation) {
    #expect(abs(b.gainSoftDB - soft) < 1e-9, sourceLocation: sourceLocation)
    #expect(abs(b.gainLoudDB - loud) < 1e-9, sourceLocation: sourceLocation)
    #expect(abs(b.compressionRatio - cr) < 1e-9, sourceLocation: sourceLocation)
}

@Test func normalHearingIsUnity() {
    let bands = FittingRule.bands(for: HearingProfile.flat(), ear: .left, strength: 1)
    #expect(bands.count == 12)
    for b in bands { expectFitting(b, 0, 0, 1) }
}

@Test func userLeftAt1500() {
    let bands = FittingRule.bands(for: userProfile(), ear: .left, strength: 1)
    expectFitting(bands[5], 39, 16.25, 45 / 22.25)
}

@Test func userLeftAt6k() {
    let bands = FittingRule.bands(for: userProfile(), ear: .left, strength: 1)
    expectFitting(bands[9], 60, 25, 4.5)
}

@Test func user12kHoldsAt8k() {
    let p = userProfile()
    for ear in Ear.allCases {
        let bands = FittingRule.bands(for: p, ear: ear, strength: 1)
        #expect(bands[11] == bands[10])
    }
}

@Test func userRightAt500() {
    let bands = FittingRule.bands(for: userProfile(), ear: .right, strength: 1)
    expectFitting(bands[2], 7, 0, 45 / 38)
}

@Test func lowFrequencyCorrection() {
    let bands = FittingRule.bands(for: uniformProfile(40), ear: .left, strength: 1)
    expectFitting(bands[1], 14, 0, bands[1].compressionRatio)
    expectFitting(bands[2], 19, 5, bands[2].compressionRatio)
}

@Test func ratioClampedAt10() {
    let bands = FittingRule.bands(for: uniformProfile(120), ear: .left, strength: 1)
    expectFitting(bands[4], 72, 30, 10)
}

@Test func strengthScales() {
    let p = userProfile()
    for b in FittingRule.bands(for: p, ear: .left, strength: 0) { expectFitting(b, 0, 0, 1) }
    let half = FittingRule.bands(for: p, ear: .left, strength: 0.5)
    #expect(abs(half[9].gainSoftDB - 28.5) < 1e-9)
    #expect(abs(half[9].gainLoudDB - 11.875) < 1e-9)
}

@Test func trimAddsToBoth() {
    var p = userProfile()
    var t = p.trim(.left)
    t[4] = 5
    p.setTrim(t, for: .left)
    let left = FittingRule.bands(for: p, ear: .left, strength: 1)
    expectFitting(left[4], 38, 18.75, 45 / (45 - 19.25))
    let right = FittingRule.bands(for: p, ear: .right, strength: 1)
    let original = FittingRule.bands(for: userProfile(), ear: .right, strength: 1)
    #expect(right == original)
}

@Test func interpolationLogFrequency() {
    let a = Audiogram(points: [1000: 20, 2000: 60, 4000: 60, 8000: 60].sorted { $0.key < $1.key }
        .map { AudiogramPoint(frequencyHz: $0.key, thresholdDBHL: $0.value) })
    #expect(abs(FittingRule.interpolatedHL(a, at: 1500) - 43.40) < 0.01)
}

@Test func gainCurveMatchesRust() {
    let b = BandFitting(gainSoftDB: 60, gainLoudDB: 25, compressionRatio: 4.5)
    #expect(abs(FittingRule.gainDB(b, inputLevelDBFS: -90) - 45) < 1e-9)
    #expect(abs(FittingRule.gainDB(b, inputLevelDBFS: -70) - 60) < 1e-9)
    #expect(abs(FittingRule.gainDB(b, inputLevelDBFS: -60) - 60) < 1e-9)
    #expect(abs(FittingRule.gainDB(b, inputLevelDBFS: -15) - 25) < 1e-9)
}

@Test func paramsPacking() {
    let p = FittingRule.params(profile: userProfile(), strength: 1, mpoDBFS: -3, outputGainDB: -6, mode: .music)
    let b = p.left.6
    #expect(abs(b.gain_soft_db - 42) < 1e-9)
    #expect(abs(b.gain_loud_db - 17.5) < 1e-9)
    #expect(abs(b.compression_ratio - 45 / 20.5) < 1e-9)
    #expect(abs(p.mpo_dbfs - -3) < 1e-9)
    #expect(abs(p.output_gain_db - -6) < 1e-9)
    #expect(abs(p.attack_ms - 10) < 1e-9)
    #expect(abs(p.release_ms - 200) < 1e-9)
}

@Test func nilProfileIsUnity() {
    let p = FittingRule.params(profile: nil, strength: 1, mpoDBFS: -3, outputGainDB: 0, mode: .speech)
    let all = withUnsafeBytes(of: p.left) { Array($0.bindMemory(to: ac_band_fitting.self)) }
        + withUnsafeBytes(of: p.right) { Array($0.bindMemory(to: ac_band_fitting.self)) }
    #expect(all.count == 24)
    for b in all {
        #expect(b.gain_soft_db == 0 && b.gain_loud_db == 0 && b.compression_ratio == 1)
    }
    #expect(p.attack_ms == 5 && p.release_ms == 50)
}
