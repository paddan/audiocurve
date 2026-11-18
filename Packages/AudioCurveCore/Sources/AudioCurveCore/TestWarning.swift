import Foundation

/// A warning on the hearing test's result screen: the result's flags, with the per-frequency
/// flags grouped into one warning per ear. The UI turns each into localized text.
public enum TestWarning: Equatable, Sendable {
    case retestMismatch(Ear, dB: Double)
    case catchTrialPresses
    case noResponseAtMax(Ear, frequenciesHz: [Double])
    case notConverged(Ear, frequenciesHz: [Double])
    case asymmetry

    /// Fixed order: retest (right ear first), catch-trial presses, no response at max (right, left),
    /// not converged (right, left), asymmetry. Frequencies ascending, each once.
    public static func from(_ flags: [TestFlag]) -> [TestWarning] {
        let ears: [Ear] = [.right, .left]
        var retest: [Ear: Double] = [:]
        var catchPresses = false
        var noResponse: [Ear: Set<Double>] = [:]
        var notConverged: [Ear: Set<Double>] = [:]
        var asymmetry = false
        for flag in flags {
            switch flag {
            case let .retestMismatch(ear, dB): retest[ear] = max(retest[ear] ?? dB, dB)
            case .catchTrialPresses: catchPresses = true
            case let .noResponseAtMax(ear, f): noResponse[ear, default: []].insert(f)
            case let .notConverged(ear, f): notConverged[ear, default: []].insert(f)
            case .asymmetry: asymmetry = true
            }
        }
        var out: [TestWarning] = []
        for ear in ears { if let dB = retest[ear] { out.append(.retestMismatch(ear, dB: dB)) } }
        if catchPresses { out.append(.catchTrialPresses) }
        for ear in ears {
            if let fs = noResponse[ear] { out.append(.noResponseAtMax(ear, frequenciesHz: fs.sorted())) }
        }
        for ear in ears {
            if let fs = notConverged[ear] { out.append(.notConverged(ear, frequenciesHz: fs.sorted())) }
        }
        if asymmetry { out.append(.asymmetry) }
        return out
    }
}

extension HearingTestRecord {
    /// "<device> – <date>", the suggested name for a measured profile.
    public static func defaultName(deviceName: String, date: Date, locale: Locale = .current,
                                   timeZone: TimeZone = .current) -> String {
        let style = Date.FormatStyle(date: .abbreviated, time: .omitted, locale: locale, timeZone: timeZone)
        return "\(deviceName) – \(date.formatted(style))"
    }
}
