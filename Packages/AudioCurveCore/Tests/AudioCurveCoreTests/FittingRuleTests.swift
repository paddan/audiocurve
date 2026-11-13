import Foundation
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

private struct PrescriptionCase: Decodable {
    let prescription: String
    let f: Double, hl: Double, tone: Double, soft: Double, loud: Double, cr: Double
}

@Test func prescriptionCasesMatchFixture() throws {
    let url = try #require(Bundle.module.url(forResource: "prescription-cases", withExtension: "json", subdirectory: "Fixtures"))
    let rows = try JSONDecoder().decode([PrescriptionCase].self, from: Data(contentsOf: url))
    #expect(rows.count >= 12)
    for r in rows {
        let rx: Prescription = r.prescription == "speech" ? .speech : .music
        let bands = FittingRule.bands(for: uniformProfile(r.hl), ear: .left, strength: 1, tone: r.tone, prescription: rx)
        let i = try #require(Bands.centers.firstIndex(of: r.f))
        #expect(abs(bands[i].gainSoftDB - r.soft) < 0.001, "soft \(r)")
        #expect(abs(bands[i].gainLoudDB - r.loud) < 0.001, "loud \(r)")
        #expect(abs(bands[i].compressionRatio - r.cr) < 0.001, "cr \(r)")
    }
}

@Test func normalHearingIsUnity() {
    for rx in [Prescription.speech, .music] {
        let bands = FittingRule.bands(for: HearingProfile.flat(), ear: .left, strength: 1, prescription: rx)
        #expect(bands.count == 12)
        for b in bands { expectFitting(b, 0, 0, 1) }
    }
}

@Test func speechRatioAtMost3() {
    for hl in stride(from: 0.0, through: 120, by: 10) {
        for b in FittingRule.bands(for: uniformProfile(hl), ear: .left, strength: 1, prescription: .speech) {
            #expect(b.compressionRatio >= 1 && b.compressionRatio <= 3)
        }
    }
}

@Test func musicRatioAtMost2AndNoBassCut() {
    for hl in stride(from: 0.0, through: 120, by: 10) {
        let music = FittingRule.bands(for: uniformProfile(hl), ear: .left, strength: 1, prescription: .music)
        let hle = hl <= 60 ? hl : 60 + 0.5 * (hl - 60)
        for i in 0..<Bands.count {
            #expect(music[i].compressionRatio >= 1 && music[i].compressionRatio <= 2)
            if Bands.centers[i] <= 500 {
                // No low-frequency correction is subtracted.
                #expect(abs(music[i].gainSoftDB - 0.45 * hle) < 1e-9)
                #expect(abs(music[i].gainLoudDB - 0.3 * hle) < 1e-9)
            }
        }
    }
}

@Test func toneZeroIsBase() {
    let p = exampleProfile()
    for rx in [Prescription.speech, .music] {
        #expect(FittingRule.bands(for: p, ear: .left, strength: 1, tone: 0, prescription: rx)
                == FittingRule.bands(for: p, ear: .left, strength: 1, prescription: rx))
    }
}

@Test func toneKeepsSoftTrebleAndLowersLoudBy6() {
    let p = uniformProfile(60)
    for rx in [Prescription.speech, .music] {
        let base = FittingRule.bands(for: p, ear: .left, strength: 1, tone: 0, prescription: rx)
        let full = FittingRule.bands(for: p, ear: .left, strength: 1, tone: 1, prescription: rx)
        for i in 0..<Bands.count where Bands.centers[i] > 2000 {
            #expect(full[i].gainSoftDB == base[i].gainSoftDB)
            #expect(abs(full[i].gainLoudDB - (base[i].gainLoudDB - 6)) < 1e-9)
        }
        for i in 0..<Bands.count where Bands.centers[i] <= 250 {
            #expect(abs(full[i].gainSoftDB - (base[i].gainSoftDB + 5)) < 1e-9)
            #expect(abs(full[i].gainLoudDB - (base[i].gainLoudDB + 5)) < 1e-9)
        }
    }
}

@Test func strengthZeroToneFullIsFiniteAndNonNegative() {
    for hl in stride(from: 0.0, through: 120, by: 10) {
        for rx in [Prescription.speech, .music] {
            for s in [0.0, 0.5, 1.0] {
                for b in FittingRule.bands(for: uniformProfile(hl), ear: .left, strength: s, tone: 1, prescription: rx) {
                    #expect(b.gainSoftDB.isFinite && b.gainSoftDB >= 0)
                    #expect(b.gainLoudDB.isFinite && b.gainLoudDB >= 0)
                    #expect(b.compressionRatio.isFinite && b.compressionRatio >= 1 && b.compressionRatio <= (rx == .speech ? 3 : 2))
                }
            }
        }
    }
}

@Test func example12kHoldsAt8k() {
    let p = exampleProfile()
    for ear in Ear.allCases {
        for rx in [Prescription.speech, .music] {
            let bands = FittingRule.bands(for: p, ear: ear, strength: 1, prescription: rx)
            #expect(bands[11] == bands[10])
        }
    }
}

@Test func exampleLeftAt1500() {
    // HL 65 -> effective 62.5.
    let b = FittingRule.bands(for: exampleProfile(), ear: .left, strength: 1, prescription: .speech)[5]
    expectFitting(b, 37.5, 15.625, 60 / (60 - 21.875))
    let m = FittingRule.bands(for: exampleProfile(), ear: .left, strength: 1, prescription: .music)[5]
    expectFitting(m, 0.45 * 62.5, 0.3 * 62.5, 60 / (60 - 0.15 * 62.5))
}

@Test func strengthScalesAndZeroIsUnity() {
    let p = exampleProfile()
    for b in FittingRule.bands(for: p, ear: .left, strength: 0, prescription: .speech) { expectFitting(b, 0, 0, 1) }
    let full = FittingRule.bands(for: p, ear: .left, strength: 1, prescription: .speech)
    let half = FittingRule.bands(for: p, ear: .left, strength: 0.5, prescription: .speech)
    #expect(abs(half[9].gainSoftDB - full[9].gainSoftDB / 2) < 1e-9)
    #expect(abs(half[9].gainLoudDB - full[9].gainLoudDB / 2) < 1e-9)
}

@Test func nanStrengthIsZero() {
    let bands = FittingRule.bands(for: exampleProfile(), ear: .left, strength: .nan, prescription: .speech)
    for b in bands { expectFitting(b, 0, 0, 1) }
}

@Test func shortTrimArrayDoesNotCrash() {
    var p = exampleProfile()
    p.leftTrimDB = []
    let bands = FittingRule.bands(for: p, ear: .left, strength: 1, prescription: .speech)
    #expect(bands.count == 12)
    #expect(bands == FittingRule.bands(for: exampleProfile(), ear: .left, strength: 1, prescription: .speech))
}

@Test func trimAddsToBoth() {
    var p = exampleProfile()
    var t = p.trim(.left)
    t[4] = 5
    p.setTrim(t, for: .left)
    let base = FittingRule.bands(for: exampleProfile(), ear: .left, strength: 1, prescription: .speech)
    let left = FittingRule.bands(for: p, ear: .left, strength: 1, prescription: .speech)
    #expect(abs(left[4].gainSoftDB - (base[4].gainSoftDB + 5)) < 1e-9)
    #expect(abs(left[4].gainLoudDB - (base[4].gainLoudDB + 5)) < 1e-9)
    let right = FittingRule.bands(for: p, ear: .right, strength: 1, prescription: .speech)
    #expect(right == FittingRule.bands(for: exampleProfile(), ear: .right, strength: 1, prescription: .speech))
}

@Test func blendIsLinearInGains() {
    let sp = FittingRule.bands(for: exampleProfile(), ear: .left, strength: 1, prescription: .speech)
    let mu = FittingRule.bands(for: exampleProfile(), ear: .left, strength: 1, prescription: .music)
    #expect(FittingRule.blend(sp, mu, weight: 1) == sp)
    #expect(FittingRule.blend(sp, mu, weight: 0) == mu)
    let h = FittingRule.blend(sp, mu, weight: 0.5)
    for i in 0..<Bands.count {
        #expect(abs(h[i].gainSoftDB - (sp[i].gainSoftDB + mu[i].gainSoftDB) / 2) < 1e-9)
        #expect(abs(h[i].gainLoudDB - (sp[i].gainLoudDB + mu[i].gainLoudDB) / 2) < 1e-9)
    }
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
    let profile = exampleProfile()
    let p = FittingRule.params(profile: profile, strength: 1, tone: 0.5, mpoDBFS: -3, outputGainDB: -6, mode: .music)
    func check(_ raw: [ac_band_fitting], _ expected: [BandFitting]) {
        for i in 0..<Bands.count {
            #expect(raw[i].gain_soft_db == expected[i].gainSoftDB)
            #expect(raw[i].gain_loud_db == expected[i].gainLoudDB)
            #expect(raw[i].compression_ratio == expected[i].compressionRatio)
        }
    }
    func arr(_ set: UnsafeRawBufferPointer) -> [ac_band_fitting] { Array(set.bindMemory(to: ac_band_fitting.self)) }
    for (ear, speechSet, musicSet) in [(Ear.left, p.left, p.music_left), (Ear.right, p.right, p.music_right)] {
        check(withUnsafeBytes(of: speechSet) { arr($0) },
              FittingRule.bands(for: profile, ear: ear, strength: 1, tone: 0.5, prescription: .speech))
        check(withUnsafeBytes(of: musicSet) { arr($0) },
              FittingRule.bands(for: profile, ear: ear, strength: 1, tone: 0.5, prescription: .music))
    }
    #expect(abs(p.mpo_dbfs - -3) < 1e-9)
    #expect(abs(p.output_gain_db - -6) < 1e-9)
    #expect(p.speech_weight == 0)
}

@Test func nilProfileIsUnity() {
    let p = FittingRule.params(profile: nil, strength: 1, mpoDBFS: -3, outputGainDB: 0, mode: .speech)
    let all = withUnsafeBytes(of: p.left) { Array($0.bindMemory(to: ac_band_fitting.self)) }
        + withUnsafeBytes(of: p.right) { Array($0.bindMemory(to: ac_band_fitting.self)) }
    #expect(all.count == 24)
    for b in all {
        #expect(b.gain_soft_db == 0 && b.gain_loud_db == 0 && b.compression_ratio == 1)
    }
    #expect(p.speech_weight == 1)
}

@Test func hostParametersAreClamped() {
    func p(_ mpo: Double, _ gain: Double) -> ac_params {
        FittingRule.params(profile: nil, strength: 0.5, mpoDBFS: mpo, outputGainDB: gain, mode: .speech)
    }
    #expect(p(5, 100).mpo_dbfs == -1)
    #expect(p(5, 100).output_gain_db == 12)
    #expect(p(-100, -100).mpo_dbfs == -20)
    #expect(p(-100, -100).output_gain_db == -48)
    #expect(p(-5, 3).mpo_dbfs == -5)
    #expect(p(-5, 3).output_gain_db == 3)
    #expect(p(.nan, .nan).mpo_dbfs == -3)
    #expect(p(.nan, .nan).output_gain_db == 0)
    #expect(p(.infinity, -.infinity).mpo_dbfs == -1)
    #expect(p(.infinity, -.infinity).output_gain_db == -48)
}

@Test func compressionOffGivesFixedLoudGain() {
    let p = FittingRule.params(profile: exampleProfile(), strength: 0.5, mpoDBFS: -3, outputGainDB: 0,
                               mode: .speech, compression: false)
    let expected = FittingRule.bands(for: exampleProfile(), ear: .left, strength: 0.5, prescription: .speech)
    let expectedMusic = FittingRule.bands(for: exampleProfile(), ear: .left, strength: 0.5, prescription: .music)
    for (set, want) in [(p.left, expected), (p.music_left, expectedMusic)] {
        withUnsafeBytes(of: set) { raw in
            let b = raw.bindMemory(to: ac_band_fitting.self)
            for i in 0..<Bands.count {
                #expect(b[i].compression_ratio == 1)
                #expect(b[i].gain_soft_db == want[i].gainLoudDB)
                #expect(b[i].gain_loud_db == want[i].gainLoudDB)
            }
        }
    }
    #expect(p.limiter_enabled == 1)
}

@Test func limiterFlagPassesThrough() {
    let p = FittingRule.params(profile: nil, strength: 0.5, mpoDBFS: -3, outputGainDB: 0, mode: .music, limiter: false)
    #expect(p.limiter_enabled == 0)
}

@Test func inputGainPassesThroughClamped() {
    let p = FittingRule.params(profile: nil, strength: 0.5, mpoDBFS: -3, outputGainDB: 0, mode: .music, inputGainDB: -12)
    #expect(p.input_gain_db == -12)
    let lo = FittingRule.params(profile: nil, strength: 0.5, mpoDBFS: -3, outputGainDB: 0, mode: .music, inputGainDB: -99)
    #expect(lo.input_gain_db == -30)
    let nan = FittingRule.params(profile: nil, strength: 0.5, mpoDBFS: -3, outputGainDB: 0, mode: .music, inputGainDB: .nan)
    #expect(nan.input_gain_db == 0)
    let def = FittingRule.params(profile: nil, strength: 0.5, mpoDBFS: -3, outputGainDB: 0, mode: .music)
    #expect(def.input_gain_db == 0)
}

@Test func autoModeSetsFlagWithZeroSpeechWeight() {
    let a = FittingRule.params(profile: nil, strength: 0.5, mpoDBFS: -3, outputGainDB: 0, mode: .auto)
    #expect(a.auto_mode == 1)
    #expect(a.speech_weight == 0)
    let s = FittingRule.params(profile: nil, strength: 0.5, mpoDBFS: -3, outputGainDB: 0, mode: .speech)
    #expect(s.auto_mode == 0)
    #expect(s.speech_weight == 1)
}
