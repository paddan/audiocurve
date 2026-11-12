import Foundation
import CAudioCurveDSP

public enum AutoLevelResult: Equatable, Sendable {
    case tooQuiet
    case gain(Double)
}

/// Accumulates meter blocks and derives an output gain: the processed level matches the input
/// level, but never so high that the limiter has to work — loudness beyond that belongs to the
/// computer's volume, not to Utnivå.
public struct AutoLevelMeasurement: Sendable {
    private var inPowerSum: Double = 0
    private var outPowerSum: Double = 0
    private var sampleCount: Int = 0
    /// Per block, the louder ear: output peak (dBFS) and safety-stage reduction (dB).
    private var peaks: [Double] = []
    private var reductions: [Double] = []

    /// Peaks are kept this far under Tak.
    static let marginDB = 1.0
    /// Fraction of blocks allowed to exceed the statistics (short spikes are ignored).
    static let quantile = 0.9

    public init() {}

    /// Adds one meter block. Both channels count as one sample each.
    public mutating func add(_ m: ac_meters) {
        let inDB = [Double(m.in_rms_db.0), Double(m.in_rms_db.1)]
        let outDB = [Double(m.out_rms_db.0), Double(m.out_rms_db.1)]
        for db in inDB { inPowerSum += pow(10, db / 10) }
        for db in outDB { outPowerSum += pow(10, db / 10) }
        sampleCount += 2
        peaks.append(Double(max(m.out_peak_db.0, m.out_peak_db.1)))
        reductions.append(Double(max(0, max(m.reduction_db.0, m.reduction_db.1))))
    }

    /// Input below −50 dBFS gives `.tooQuiet`. Otherwise the smaller of
    /// - loudness match: `current + in − out` (power means over all channels and blocks), and
    /// - limiter-idle: `current + (mpo − margin − peak90) − reduction90`, which lowers the gain
    ///   by what the limiter takes away and never lifts typical peaks into Tak,
    /// clamped to −48…12.
    public func result(currentOutputGainDB: Double, mpoDBFS: Double) -> AutoLevelResult {
        guard sampleCount > 0 else { return .tooQuiet }
        let count = Double(sampleCount)
        let inDB = 10 * log10(inPowerSum / count)
        let outDB = 10 * log10(outPowerSum / count)
        guard inDB >= -50 else { return .tooQuiet }
        let match = currentOutputGainDB + inDB - outDB
        let idle = currentOutputGainDB + (mpoDBFS - Self.marginDB - Self.percentile(peaks))
            - Self.percentile(reductions)
        return .gain(min(12, max(-48, min(match, idle))))
    }

    private static func percentile(_ v: [Double]) -> Double {
        guard !v.isEmpty else { return 0 }
        let s = v.filter(\.isFinite).sorted()
        guard !s.isEmpty else { return 0 }
        return s[Int((Double(s.count - 1) * quantile).rounded(.down))]
    }
}
