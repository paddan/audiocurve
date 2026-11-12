import Testing
import CAudioCurveDSP
@testable import AudioCurveCore

/// One meter block with the same value on both channels (peak 10 dB above RMS by default).
private func meters(inDB: Float, outDB: Float, peakDB: Float? = nil, reductionDB: Float = 0) -> ac_meters {
    var m = ac_meters()
    m.in_rms_db = (inDB, inDB)
    m.out_rms_db = (outDB, outDB)
    let pk = peakDB ?? outDB + 10
    m.out_peak_db = (pk, pk)
    m.reduction_db = (reductionDB, reductionDB)
    return m
}

@Test func autoLevelMatches() {
    var m = AutoLevelMeasurement()
    for _ in 0..<60 { m.add(meters(inDB: -20, outDB: -14)) }
    guard case let .gain(g) = m.result(currentOutputGainDB: 0, mpoDBFS: -3) else {
        Issue.record("expected a gain result")
        return
    }
    #expect(abs(g - (-6)) < 0.01)
}

@Test func autoLevelClamps() {
    var m = AutoLevelMeasurement()
    for _ in 0..<60 { m.add(meters(inDB: -20, outDB: -80)) }
    #expect(m.result(currentOutputGainDB: 0, mpoDBFS: -3) == .gain(12))
}

@Test func autoLevelTooQuiet() {
    var m = AutoLevelMeasurement()
    for _ in 0..<60 { m.add(meters(inDB: -60, outDB: -60)) }
    #expect(m.result(currentOutputGainDB: 0, mpoDBFS: -3) == .tooQuiet)
}

@Test func autoLevelNoSamplesIsTooQuiet() {
    let m = AutoLevelMeasurement()
    #expect(m.result(currentOutputGainDB: 0, mpoDBFS: -3) == .tooQuiet)
}

@Test func autoLevelPowerMean() {
    var m = AutoLevelMeasurement()
    for _ in 0..<30 { m.add(meters(inDB: -20, outDB: -20)) }
    for _ in 0..<30 { m.add(meters(inDB: -30, outDB: -30)) }
    guard case let .gain(g) = m.result(currentOutputGainDB: 0, mpoDBFS: -3) else {
        Issue.record("expected a gain result")
        return
    }
    #expect(abs(g) < 0.01)
}

@Test func autoLevelBacksOffWhenLimiterWorks() {
    // Loudness match alone says -6, but the limiter is pressing ~10 dB at the ceiling:
    // back off by the reduction plus 1 dB margin under Tak.
    var m = AutoLevelMeasurement()
    for _ in 0..<60 { m.add(meters(inDB: -20, outDB: -14, peakDB: -3, reductionDB: 10)) }
    guard case let .gain(g) = m.result(currentOutputGainDB: 0, mpoDBFS: -3) else {
        Issue.record("expected a gain result"); return
    }
    #expect(abs(g - (-11)) < 0.01, "\(g)")
}

@Test func autoLevelNeverRaisesPeaksIntoTak() {
    // Loudness match wants +6, but peaks are already at -5 with Tak -3: only +1 fits.
    var m = AutoLevelMeasurement()
    for _ in 0..<60 { m.add(meters(inDB: -20, outDB: -26, peakDB: -5)) }
    guard case let .gain(g) = m.result(currentOutputGainDB: 0, mpoDBFS: -3) else {
        Issue.record("expected a gain result"); return
    }
    #expect(abs(g - 1) < 0.01, "\(g)")
}

@Test func autoLevelIgnoresRareReductionSpikes() {
    // A few blocks of reduction (under 10 %) do not pull the level down.
    var m = AutoLevelMeasurement()
    for i in 0..<60 { m.add(meters(inDB: -20, outDB: -20, peakDB: -10, reductionDB: i < 4 ? 8 : 0)) }
    guard case let .gain(g) = m.result(currentOutputGainDB: 0, mpoDBFS: -3) else {
        Issue.record("expected a gain result"); return
    }
    #expect(abs(g) < 0.01, "\(g)")
}
