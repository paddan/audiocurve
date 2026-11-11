import Testing
@testable import AudioCurveCore

let userLeft: [Double: Double] = [125: 20, 250: 25, 500: 35, 750: 40, 1000: 55, 1500: 65, 2000: 70, 3000: 80, 4000: 85, 6000: 95, 8000: 105]
let userRight: [Double: Double] = [125: 15, 250: 15, 500: 20, 750: 25, 1000: 35, 1500: 45, 2000: 50, 3000: 60, 4000: 65, 6000: 75, 8000: 85]

func audiogram(from dict: [Double: Double]) -> Audiogram {
    Audiogram(points: dict.sorted { $0.key < $1.key }.map { AudiogramPoint(frequencyHz: $0.key, thresholdDBHL: $0.value) })
}

func userProfile() -> HearingProfile {
    HearingProfile(left: audiogram(from: userLeft), right: audiogram(from: userRight),
                   leftTrimDB: Array(repeating: 0, count: 12), rightTrimDB: Array(repeating: 0, count: 12))
}

func validationError(_ p: HearingProfile) -> ProfileValidationError? {
    do { try p.validate(); return nil } catch { return error }
}
