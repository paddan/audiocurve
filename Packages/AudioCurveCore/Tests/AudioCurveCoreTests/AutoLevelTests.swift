import Testing
import CAudioCurveDSP
@testable import AudioCurveCore

/// One meter block with the same value on both channels.
private func meters(inDB: Float, outDB: Float) -> ac_meters {
    var m = ac_meters()
    m.in_rms_db = (inDB, inDB)
    m.out_rms_db = (outDB, outDB)
    return m
}

@Test func autoLevelMatches() {
    var m = AutoLevelMeasurement()
    for _ in 0..<60 { m.add(meters(inDB: -20, outDB: -14)) }
    guard case let .gain(g) = m.result(currentOutputGainDB: 0) else {
        Issue.record("expected a gain result")
        return
    }
    #expect(abs(g - (-6)) < 0.01)
}

@Test func autoLevelClamps() {
    var m = AutoLevelMeasurement()
    for _ in 0..<60 { m.add(meters(inDB: -20, outDB: -80)) }
    #expect(m.result(currentOutputGainDB: 0) == .gain(12))
}

@Test func autoLevelTooQuiet() {
    var m = AutoLevelMeasurement()
    for _ in 0..<60 { m.add(meters(inDB: -60, outDB: -60)) }
    #expect(m.result(currentOutputGainDB: 0) == .tooQuiet)
}

@Test func autoLevelNoSamplesIsTooQuiet() {
    let m = AutoLevelMeasurement()
    #expect(m.result(currentOutputGainDB: 0) == .tooQuiet)
}

@Test func autoLevelPowerMean() {
    var m = AutoLevelMeasurement()
    for _ in 0..<30 { m.add(meters(inDB: -20, outDB: -20)) }
    for _ in 0..<30 { m.add(meters(inDB: -30, outDB: -30)) }
    guard case let .gain(g) = m.result(currentOutputGainDB: 0) else {
        Issue.record("expected a gain result")
        return
    }
    #expect(abs(g) < 0.01)
}
