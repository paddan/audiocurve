import Testing
@testable import AudioCurveCore

private let calibration = TestCalibration.nominal(.inEar)
private let noCatch: @Sendable () -> Double = { 0.5 }

private struct Run {
    var presentations: [Presentation]
    var result: HearingTestResult
}

/// Drives a test to the end. `listener` sees each presentation and the number of
/// presentations so far (including this one) and says whether the tone was heard.
private func drive(
    calibration: TestCalibration = calibration,
    random: @escaping @Sendable () -> Double = noCatch,
    listener: (Presentation, Int) -> Bool
) -> Run {
    var test = HearingTest(calibration: calibration, random: random)
    var seen: [Presentation] = []
    while true {
        switch test.next() {
        case .finished(let result):
            return Run(presentations: seen, result: result)
        case .present(let p):
            seen.append(p)
            test.respond(heard: listener(p, seen.count))
            #expect(seen.count < 2000)
            if seen.count >= 2000 { fatalError("test did not terminate") }
        }
    }
}

/// A listener with a fixed threshold per (ear, frequency).
private func simulate(
    thresholdHL: (Ear, Double) -> Double,
    random: @escaping @Sendable () -> Double = noCatch
) -> Run {
    drive(random: random) { p, _ in
        p.levelDBFS != nil && p.levelHL >= thresholdHL(p.ear, p.frequencyHz)
    }
}

@Test func findsSimulatedThresholds() {
    let run = simulate { _, _ in 35 }
    for points in [run.result.left, run.result.right] {
        #expect(points.count == 11)
        #expect(points.allSatisfy { $0.hl == 35 })
    }
    #expect(run.result.flags.isEmpty)
}

@Test func floorIsMinus10() {
    let run = simulate { _, _ in -100 }
    #expect(run.presentations.allSatisfy { $0.levelHL >= -10 })
    #expect(run.result.left.allSatisfy { $0.hl == -10 })
    #expect(run.result.right.allSatisfy { $0.hl == -10 })
}

@Test func neverExceedsCap() {
    let run = simulate { _, _ in 1000 }
    for p in run.presentations {
        #expect(p.levelDBFS! <= TestCalibration.capDBFS(p.frequencyHz))
    }
    for ear in Ear.allCases {
        let points = ear == .left ? run.result.left : run.result.right
        for point in points {
            #expect(point.hl == TestCalibration.roundedHL(calibration.maxHL(at: point.frequencyHz)))
            #expect(run.result.flags.contains(.noResponseAtMax(ear, frequencyHz: point.frequencyHz)))
        }
    }
}

@Test func familiarizationStepsAbove80() {
    let run = simulate { _, _ in 1000 }
    let first = run.presentations.prefix { $0.ear == .right && $0.frequencyHz == 1000 }
    let levels = first.map(\.levelHL)
    #expect(levels.starts(with: [40, 60, 80, 85, 90, 95]))
}

@Test func orderAndCount() {
    let run = simulate { _, _ in 30 }
    var sequence: [[Double]] = []
    var indices: [Int] = []
    for p in run.presentations {
        let key = [p.ear == .right ? 0.0 : 1.0, p.frequencyHz]
        if sequence.last != key {
            sequence.append(key)
            indices.append(p.index)
        }
    }
    let expected = [Ear.right, .left].flatMap { ear in
        HearingTest.order.map { [ear == .right ? 0.0 : 1.0, $0] }
    }
    #expect(sequence == expected)
    #expect(indices == Array(1...24))
    #expect(run.result.left.count == 11)
    #expect(run.result.right.count == 11)
    #expect(run.result.right.map(\.frequencyHz) == Audiogram.standardFrequencies)
}

@Test func catchTrialsAreSilentAndCounted() {
    // random() == 0 always asks for a catch trial, but never two in a row.
    var catchSeen = 0
    let run = drive(random: { 0 }) { p, _ in
        if p.levelDBFS == nil {
            catchSeen += 1
            return catchSeen <= 3
        }
        return p.levelHL >= 35
    }
    #expect(catchSeen > 3)
    #expect(run.presentations.contains { $0.levelDBFS == nil })
    #expect(run.result.flags.contains(.catchTrialPresses(3)))
    #expect(run.result.right.allSatisfy { $0.hl == 35 })
}

@Test func twoCatchPressesAreNotFlagged() {
    var catchSeen = 0
    let run = drive(random: { 0 }) { p, _ in
        if p.levelDBFS == nil {
            catchSeen += 1
            return catchSeen <= 2
        }
        return p.levelHL >= 35
    }
    #expect(!run.result.flags.contains { if case .catchTrialPresses = $0 { true } else { false } })
}

@Test func retestMismatchFlagged() {
    var kilohertzRuns = 0
    var lastFrequency = 0.0
    let run = drive { p, _ in
        if p.ear == .right, p.frequencyHz == 1000, lastFrequency != 1000 { kilohertzRuns += 1 }
        lastFrequency = p.ear == .right ? p.frequencyHz : 0
        let threshold = (p.ear == .right && p.frequencyHz == 1000 && kilohertzRuns == 1) ? 20.0 : 40.0
        return p.levelHL >= threshold
    }
    #expect(kilohertzRuns == 2)
    #expect(run.result.flags.contains(.retestMismatch(.right, dB: 20)))
    #expect(run.result.right.first { $0.frequencyHz == 1000 }?.hl == 20)
    #expect(!run.result.flags.contains(.retestMismatch(.left, dB: 20)))
}

@Test func notConvergedBailsOut() {
    // Heard only on descending presentations: no level ever gets 2 ascending hits.
    var previous: Presentation?
    let run = drive { p, _ in
        defer { previous = p }
        guard let previous, previous.ear == p.ear, previous.frequencyHz == p.frequencyHz else {
            return true  // first presentation of a point
        }
        return p.levelHL < previous.levelHL
    }
    var perPoint: [String: Int] = [:]
    for p in run.presentations { perPoint["\(p.ear)\(p.index)", default: 0] += 1 }
    #expect(perPoint.count == 24)
    #expect(perPoint.values.allSatisfy { $0 <= 12 })
    for ear in Ear.allCases {
        for f in Audiogram.standardFrequencies {
            #expect(run.result.flags.contains(.notConverged(ear, frequencyHz: f)))
        }
    }
}

@Test func asymmetryFlagged() {
    let run = simulate { ear, _ in ear == .right ? 10 : 60 }
    #expect(run.result.flags.filter { $0 == .asymmetry }.count == 1)
}

@Test func extraResponsesIgnored() {
    var test = HearingTest(calibration: calibration, random: noCatch)
    guard case .present(let first) = test.next() else { Issue.record("no presentation"); return }
    test.respond(heard: true)
    test.respond(heard: true)
    guard case .present(let second) = test.next() else { Issue.record("no presentation"); return }
    #expect(first.levelHL == 40)
    #expect(second.levelHL == 30)
}

@Test func nextIsIdempotent() {
    var test = HearingTest(calibration: calibration, random: noCatch)
    let a = test.next()
    let b = test.next()
    #expect(a == b)
}

@Test(arguments: [Transducer.inEar, .overEar], [10.0, 20.0, 100.0])
func neverExceedsCapWithLowFullScaleSPL(transducer: Transducer, fullScaleSPL: Double) {
    let cal = TestCalibration(transducer: transducer, fullScaleSPL: fullScaleSPL)
    let listeners: [(Presentation, Int) -> Bool] = [
        { p, _ in p.levelDBFS != nil },   // hears everything
        { _, _ in false },                // hears nothing
    ]
    for listener in listeners {
        let run = drive(calibration: cal, listener: listener)
        for p in run.presentations {
            #expect(p.levelHL <= cal.maxHL(at: p.frequencyHz))
            #expect(p.levelDBFS! <= TestCalibration.capDBFS(p.frequencyHz))
        }
    }
}
