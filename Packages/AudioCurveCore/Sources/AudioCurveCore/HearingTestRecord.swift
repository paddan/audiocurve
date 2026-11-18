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
