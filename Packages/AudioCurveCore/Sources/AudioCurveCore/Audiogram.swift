import Foundation

public enum Ear: String, Codable, CaseIterable, Sendable { case left, right }

public struct AudiogramPoint: Codable, Equatable, Sendable {
    public var frequencyHz: Double
    public var thresholdDBHL: Double

    public init(frequencyHz: Double, thresholdDBHL: Double) {
        self.frequencyHz = frequencyHz
        self.thresholdDBHL = thresholdDBHL
    }
}

public struct Audiogram: Codable, Equatable, Sendable {
    public static let standardFrequencies: [Double] = [125, 250, 500, 750, 1000, 1500, 2000, 3000, 4000, 6000, 8000]

    /// Sorted by ascending frequency.
    public var points: [AudiogramPoint]

    public init(points: [AudiogramPoint]) {
        self.points = points
    }
}
