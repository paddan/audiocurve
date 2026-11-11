import Foundation
import CAudioCurveDSP

public enum AutoLevelResult: Equatable, Sendable {
    case tooQuiet
    case gain(Double)
}

/// Accumulates meter blocks and derives an output gain that matches the processed level to the input level.
public struct AutoLevelMeasurement: Sendable {
    private var inPowerSum: Double = 0
    private var outPowerSum: Double = 0
    private var sampleCount: Int = 0

    public init() {}

    /// Adds one meter block. Both channels count as one sample each.
    public mutating func add(_ m: ac_meters) {
        let inDB = [Double(m.in_rms_db.0), Double(m.in_rms_db.1)]
        let outDB = [Double(m.out_rms_db.0), Double(m.out_rms_db.1)]
        for db in inDB { inPowerSum += pow(10, db / 10) }
        for db in outDB { outPowerSum += pow(10, db / 10) }
        sampleCount += 2
    }

    /// Power mean (effektmedel) over all channels and blocks. Input below −50 dBFS gives `.tooQuiet`,
    /// otherwise `clamp(current + in − out, −48, 12)`.
    public func result(currentOutputGainDB: Double) -> AutoLevelResult {
        guard sampleCount > 0 else { return .tooQuiet }
        let count = Double(sampleCount)
        let inDB = 10 * log10(inPowerSum / count)
        let outDB = 10 * log10(outPowerSum / count)
        guard inDB >= -50 else { return .tooQuiet }
        return .gain(min(12, max(-48, currentOutputGainDB + inDB - outDB)))
    }
}
