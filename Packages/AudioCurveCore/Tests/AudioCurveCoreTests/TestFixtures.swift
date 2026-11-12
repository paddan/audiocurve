import Testing
@testable import AudioCurveCore

/// Fictional, steeply sloping example audiogram (not a real person's).
let exampleLeft: [Double: Double] = [125: 20, 250: 25, 500: 35, 750: 40, 1000: 55, 1500: 65, 2000: 70, 3000: 80, 4000: 85, 6000: 95, 8000: 105]
let exampleRight: [Double: Double] = [125: 15, 250: 15, 500: 20, 750: 25, 1000: 35, 1500: 45, 2000: 50, 3000: 60, 4000: 65, 6000: 75, 8000: 85]

func audiogram(from dict: [Double: Double]) -> Audiogram {
    Audiogram(points: dict.sorted { $0.key < $1.key }.map { AudiogramPoint(frequencyHz: $0.key, thresholdDBHL: $0.value) })
}

func exampleProfile() -> HearingProfile {
    HearingProfile(left: audiogram(from: exampleLeft), right: audiogram(from: exampleRight),
                   leftTrimDB: Array(repeating: 0, count: 12), rightTrimDB: Array(repeating: 0, count: 12))
}

func validationError(_ p: HearingProfile) -> ProfileValidationError? {
    do { try p.validate(); return nil } catch { return error }
}
