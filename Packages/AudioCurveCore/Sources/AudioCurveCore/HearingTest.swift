import Foundation

/// One tone (or silent catch trial) to play to the listener.
public struct Presentation: Equatable, Sendable {
    public var ear: Ear
    public var frequencyHz: Double
    public var levelHL: Double
    /// nil = catch trial: stay silent.
    public var levelDBFS: Double?
    /// Silence before the tone, in seconds.
    public var gapBeforeS: Double
    /// 1-based point number (1...totalPoints), for progress.
    public var index: Int
}

public enum TestFlag: Codable, Equatable, Sendable {
    case retestMismatch(Ear, dB: Double)
    case catchTrialPresses(Int)
    case noResponseAtMax(Ear, frequencyHz: Double)
    case notConverged(Ear, frequencyHz: Double)
    case asymmetry
}

public struct TestPoint: Codable, Equatable, Sendable {
    public var frequencyHz: Double
    /// Raw presented level of the threshold.
    public var dBFS: Double
    /// Threshold in dB HL, rounded.
    public var hl: Double
}

public struct HearingTestResult: Equatable, Sendable {
    /// Sorted by frequency, one point per standard frequency.
    public var left: [TestPoint]
    public var right: [TestPoint]
    public var flags: [TestFlag]

    public func audiogram(_ ear: Ear) -> Audiogram {
        let points = ear == .left ? left : right
        return Audiogram(points: points.map {
            AudiogramPoint(frequencyHz: $0.frequencyHz, thresholdDBHL: $0.hl)
        })
    }
}

public enum HearingTestStep: Equatable, Sendable {
    case present(Presentation)
    case finished(HearingTestResult)
}

/// Clinical-style ascending/descending threshold search, 5 dB HL steps.
public struct HearingTest: Sendable {
    public static let order: [Double] = [1000, 1500, 2000, 3000, 4000, 6000, 8000, 1000, 750, 500, 250, 125]
    public static var totalPoints: Int { 24 }

    private static let ears: [Ear] = [.right, .left]
    private static let floorHL = -10.0
    private static let maxPresentationsPerPoint = 12
    private static let catchProbability = 1.0 / 8

    /// Staircase state for one (ear, frequency) point.
    private struct Staircase {
        var maxHL: Double
        var level: Double
        var ascending = false
        var heardBefore = false
        var presentations = 0
        var notHeardAtMax = 0
        var heardAtFloor = 0
        var ascendingHeardCounts: [Double: Int] = [:]
        var lowestHeard: Double?
        var lowestAscendingHeard: Double?
    }

    private struct Outcome {
        var levelHL: Double
        var noResponseAtMax = false
        var notConverged = false
    }

    private let calibration: TestCalibration
    private let random: @Sendable () -> Double

    private var position = 0
    private var staircase: Staircase
    private var pending: Presentation?
    private var pendingIsCatch = false
    private var lastWasCatch = false
    private var catchPresses = 0
    private var firstKilohertz: [Ear: Double] = [:]
    private var thresholds: [Ear: [Int: Outcome]] = [:]
    private var flags: [TestFlag] = []
    private var result: HearingTestResult?

    public init(calibration: TestCalibration, random: @escaping @Sendable () -> Double) {
        self.calibration = calibration
        self.random = random
        self.staircase = Self.makeStaircase(calibration, frequencyHz: Self.order[0])
    }

    private static func makeStaircase(_ calibration: TestCalibration, frequencyHz: Double) -> Staircase {
        let maxHL = calibration.maxHL(at: frequencyHz)
        return Staircase(maxHL: maxHL, level: min(40, maxHL))
    }

    private var ear: Ear { Self.ears[position / Self.order.count] }
    private var frequencyHz: Double { Self.order[position % Self.order.count] }

    /// The current presentation (the same one until it is answered), or the result.
    public mutating func next() -> HearingTestStep {
        if let result { return .finished(result) }
        if let pending { return .present(pending) }

        let isCatch = !lastWasCatch && random() < Self.catchProbability
        let gap = 1.5 + 2 * random()
        pendingIsCatch = isCatch
        let p = Presentation(
            ear: ear,
            frequencyHz: frequencyHz,
            levelHL: staircase.level,
            levelDBFS: isCatch ? nil : min(
                TestCalibration.capDBFS(frequencyHz),
                calibration.dBFS(fromHL: staircase.level, at: frequencyHz)),
            gapBeforeS: gap,
            index: position + 1)
        pending = p
        return .present(p)
    }

    /// Answers the current presentation. Ignored when there is none outstanding.
    public mutating func respond(heard: Bool) {
        guard pending != nil else { return }
        pending = nil
        lastWasCatch = pendingIsCatch
        if pendingIsCatch {
            if heard { catchPresses += 1 }
            return
        }
        if let outcome = advance(heard: heard) {
            finishPoint(outcome)
        }
    }

    /// Moves the staircase; returns the outcome once the point is resolved.
    private mutating func advance(heard: Bool) -> Outcome? {
        var s = staircase
        let level = s.level
        s.presentations += 1
        var outcome: Outcome?

        if heard {
            s.heardBefore = true
            s.lowestHeard = min(s.lowestHeard ?? level, level)
            if s.ascending {
                s.ascendingHeardCounts[level, default: 0] += 1
                s.lowestAscendingHeard = min(s.lowestAscendingHeard ?? level, level)
                if s.ascendingHeardCounts[level, default: 0] >= 2 { outcome = Outcome(levelHL: level) }
            }
            if level <= Self.floorHL {
                s.heardAtFloor += 1
                if outcome == nil, s.heardAtFloor >= 2 { outcome = Outcome(levelHL: level) }
            }
            s.level = min(s.maxHL, max(Self.floorHL, level - 10))
            s.ascending = false
        } else {
            if level >= s.maxHL {
                s.notHeardAtMax += 1
                if s.notHeardAtMax >= 2 { outcome = Outcome(levelHL: s.maxHL, noResponseAtMax: true) }
                s.level = s.maxHL
                s.ascending = true
            } else if s.heardBefore {
                s.level = min(s.maxHL, level + 5)
                s.ascending = true
            } else {
                let step: Double = level >= 80 ? 5 : 20  // steps above 80 are +5
                s.level = min(s.maxHL, level + step)
                s.ascending = step == 5
            }
        }

        if outcome == nil, s.presentations >= Self.maxPresentationsPerPoint {
            outcome = Outcome(
                levelHL: s.lowestAscendingHeard ?? s.lowestHeard ?? s.maxHL,
                notConverged: true)
        }
        staircase = s
        return outcome
    }

    private mutating func finishPoint(_ outcome: Outcome) {
        let ear = self.ear
        let f = frequencyHz
        thresholds[ear, default: [:]][position % Self.order.count] = outcome
        if outcome.noResponseAtMax { flags.append(.noResponseAtMax(ear, frequencyHz: f)) }
        if outcome.notConverged { flags.append(.notConverged(ear, frequencyHz: f)) }

        // Retest: the second 1 kHz run (order index 7) against the first (index 0).
        if position % Self.order.count == 7,
           let first = thresholds[ear]?[0]?.levelHL {
            let diff = abs(first - outcome.levelHL)
            if diff > 10 { flags.append(.retestMismatch(ear, dB: diff)) }
        }

        position += 1
        if position == Self.totalPoints {
            result = makeResult()
        } else {
            staircase = Self.makeStaircase(calibration, frequencyHz: frequencyHz)
        }
    }

    private func points(for ear: Ear) -> [TestPoint] {
        let outcomes = thresholds[ear] ?? [:]
        var byFrequency: [Double: Double] = [:]
        for (i, outcome) in outcomes {
            let f = Self.order[i]
            // The 1 kHz value is the lower of the two runs.
            byFrequency[f] = min(byFrequency[f] ?? outcome.levelHL, outcome.levelHL)
        }
        return byFrequency.keys.sorted().map { f in
            let level = byFrequency[f]!
            return TestPoint(
                frequencyHz: f,
                dBFS: calibration.dBFS(fromHL: level, at: f),
                hl: TestCalibration.roundedHL(level))
        }
    }

    private func makeResult() -> HearingTestResult {
        let left = points(for: .left)
        let right = points(for: .right)
        var flags = self.flags
        if catchPresses > 2 { flags.append(.catchTrialPresses(catchPresses)) }
        let asymmetric = zip(left, right).contains { abs($0.hl - $1.hl) >= 40 }
        if asymmetric { flags.append(.asymmetry) }
        return HearingTestResult(left: left, right: right, flags: flags)
    }
}
