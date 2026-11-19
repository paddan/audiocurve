import Foundation

/// What a hearing test measured, kept with the profile it produced.
public struct HearingTestRecord: Codable, Equatable, Sendable {
    public var date: Date
    public var output: OutputDevice
    public var calibration: TestCalibration
    public var left: [TestPoint]
    public var right: [TestPoint]
    public var flags: [TestFlag]
    public var appVersion: String

    public init(date: Date, output: OutputDevice, calibration: TestCalibration,
                left: [TestPoint], right: [TestPoint], flags: [TestFlag], appVersion: String) {
        self.date = date
        self.output = output
        self.calibration = calibration
        self.left = left
        self.right = right
        self.flags = flags
        self.appVersion = appVersion
    }
}

public struct TestPoint: Codable, Equatable, Sendable {
    public var frequencyHz: Double
    /// Raw presented level of the threshold.
    public var dBFS: Double
    /// Threshold in dB HL, rounded.
    public var hl: Double

    public init(frequencyHz: Double, dBFS: Double, hl: Double) {
        self.frequencyHz = frequencyHz
        self.dBFS = dBFS
        self.hl = hl
    }
}

public enum TestFlag: Codable, Equatable, Sendable {
    case retestMismatch(Ear, dB: Double)
    case catchTrialPresses(Int)
    case noResponseAtMax(Ear, frequencyHz: Double)
    case notConverged(Ear, frequencyHz: Double)
    case asymmetry
}
