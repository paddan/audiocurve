import Foundation

public enum ProfileValidationError: Error, Equatable {
    case tooFewPoints(Ear)
    case unsortedOrDuplicateFrequencies(Ear)
    case nonStandardFrequency(Ear, Double)
    case thresholdOutOfRange(Ear, frequencyHz: Double)
    case wrongTrimCount(Ear)
    case trimOutOfRange(Ear, band: Int)
}

public struct HearingProfile: Codable, Equatable, Sendable {
    public var left: Audiogram
    public var right: Audiogram
    /// Per-band trim in dB, 12 values, range ±20.
    public var leftTrimDB: [Double]
    public var rightTrimDB: [Double]

    public init(left: Audiogram, right: Audiogram, leftTrimDB: [Double], rightTrimDB: [Double]) {
        self.left = left
        self.right = right
        self.leftTrimDB = leftTrimDB
        self.rightTrimDB = rightTrimDB
    }

    /// Normal hearing: 0 dB HL at all standard frequencies for both ears, no trim.
    public static func flat() -> HearingProfile {
        let points = Audiogram.standardFrequencies.map { AudiogramPoint(frequencyHz: $0, thresholdDBHL: 0) }
        let zeros = Array(repeating: 0.0, count: Bands.count)
        return HearingProfile(left: Audiogram(points: points), right: Audiogram(points: points),
                              leftTrimDB: zeros, rightTrimDB: zeros)
    }

    public func audiogram(_ ear: Ear) -> Audiogram {
        switch ear {
        case .left: left
        case .right: right
        }
    }

    public mutating func setAudiogram(_ a: Audiogram, for ear: Ear) {
        switch ear {
        case .left: left = a
        case .right: right = a
        }
    }

    public func trim(_ ear: Ear) -> [Double] {
        switch ear {
        case .left: leftTrimDB
        case .right: rightTrimDB
        }
    }

    public mutating func setTrim(_ t: [Double], for ear: Ear) {
        switch ear {
        case .left: leftTrimDB = t
        case .right: rightTrimDB = t
        }
    }

    public func validate() throws(ProfileValidationError) {
        try Self.validate(audiogram: left, trim: leftTrimDB, ear: .left)
        try Self.validate(audiogram: right, trim: rightTrimDB, ear: .right)
    }

    private static func validate(audiogram a: Audiogram, trim: [Double], ear: Ear) throws(ProfileValidationError) {
        if a.points.count < 4 { throw .tooFewPoints(ear) }
        for i in 1..<a.points.count where !(a.points[i].frequencyHz > a.points[i - 1].frequencyHz) {
            throw .unsortedOrDuplicateFrequencies(ear)
        }
        for p in a.points {
            if !Audiogram.standardFrequencies.contains(p.frequencyHz) {
                throw .nonStandardFrequency(ear, p.frequencyHz)
            }
            if !(p.thresholdDBHL >= -10 && p.thresholdDBHL <= 120) {
                throw .thresholdOutOfRange(ear, frequencyHz: p.frequencyHz)
            }
        }
        if trim.count != Bands.count { throw .wrongTrimCount(ear) }
        for (band, t) in trim.enumerated() where !(t >= -20 && t <= 20) {
            throw .trimOutOfRange(ear, band: band)
        }
    }
}
