import Foundation
import CAudioCurveDSP

public struct BandFitting: Equatable, Sendable {
    public var gainSoftDB: Double
    public var gainLoudDB: Double
    public var compressionRatio: Double

    public init(gainSoftDB: Double, gainLoudDB: Double, compressionRatio: Double) {
        self.gainSoftDB = gainSoftDB
        self.gainLoudDB = gainLoudDB
        self.compressionRatio = compressionRatio
    }
}

public enum CompressionMode: Int, Sendable {
    /// `auto`: the kernel glides between the Music and Speech time constants by its classifier.
    case speech = 0, music = 1, auto = 2

    /// For `auto` these are the Music end of the glide.
    public var attackMS: Double { self == .speech ? 5 : 10 }
    public var releaseMS: Double { self == .speech ? 50 : 200 }
}

public enum FittingRule {
    /// Threshold (dB HL) at frequency `f`, log-frequency linear interpolation between
    /// measured points. Held constant outside the measured range.
    public static func interpolatedHL(_ a: Audiogram, at f: Double) -> Double {
        let pts = a.points
        guard let first = pts.first, let last = pts.last else { return 0 }
        if f <= first.frequencyHz { return first.thresholdDBHL }
        if f >= last.frequencyHz { return last.thresholdDBHL }
        for i in 1..<pts.count where f <= pts[i].frequencyHz {
            let lo = pts[i - 1], hi = pts[i]
            let t = (log(f) - log(lo.frequencyHz)) / (log(hi.frequencyHz) - log(lo.frequencyHz))
            return lo.thresholdDBHL + t * (hi.thresholdDBHL - lo.thresholdDBHL)
        }
        return last.thresholdDBHL
    }

    /// Per-band fitting for one ear. `strength` is clamped to 0...1.
    public static func bands(for p: HearingProfile, ear: Ear, strength: Double) -> [BandFitting] {
        let s = clampStrength(strength)
        let audiogram = p.audiogram(ear)
        let trim = p.trim(ear)
        return (0..<Bands.count).map { i in
            let f = Bands.centers[i]
            let hl = interpolatedHL(audiogram, at: f)
            let corr: Double
            switch f {
            case ...250: corr = 10
            case 500: corr = 5
            default: corr = 0
            }
            let t = i < trim.count ? trim[i] : 0
            let gSoft = (max(0, 0.6 * hl - corr) + t) * s
            let gLoud = (max(0, 0.25 * hl - corr) + t) * s
            let den = 45 - (gSoft - gLoud)
            let cr = den <= 0 ? 10 : min(max(45 / den, 1), 10)
            return BandFitting(gainSoftDB: gSoft, gainLoudDB: gLoud, compressionRatio: cr)
        }
    }

    /// Static gain curve in dB for an input level in dBFS. Mirrors Rust `gain_db`.
    public static func gainDB(_ b: BandFitting, inputLevelDBFS l: Double) -> Double {
        if l < -75 {
            return b.gainSoftDB - (-75 - l)
        } else if l <= -60 {
            return b.gainSoftDB
        } else {
            return b.gainSoftDB + (l + 60) * (1 / b.compressionRatio - 1)
        }
    }

    /// Builds the C parameter block. A nil profile yields unity fitting on all bands.
    /// `compression: false` gives every band a fixed gain (its loud-level gain, ratio 1:1);
    /// `limiter: false` turns off the band budget and the lookahead limiter in the kernel.
    public static func params(profile: HearingProfile?, strength: Double, mpoDBFS: Double,
                              outputGainDB: Double, mode: CompressionMode,
                              compression: Bool = true, limiter: Bool = true,
                              inputGainDB: Double = 0, active: Bool = true) -> ac_params {
        var p = ac_params()
        let s = clampStrength(strength)
        let unity = BandFitting(gainSoftDB: 0, gainLoudDB: 0, compressionRatio: 1)
        func linear(_ b: [BandFitting]) -> [BandFitting] {
            compression ? b : b.map { BandFitting(gainSoftDB: $0.gainLoudDB, gainLoudDB: $0.gainLoudDB, compressionRatio: 1) }
        }
        let left = linear(profile.map { bands(for: $0, ear: .left, strength: s) } ?? Array(repeating: unity, count: Bands.count))
        let right = linear(profile.map { bands(for: $0, ear: .right, strength: s) } ?? Array(repeating: unity, count: Bands.count))
        withUnsafeMutableBytes(of: &p.left) { raw in
            let dst = raw.bindMemory(to: ac_band_fitting.self)
            for i in 0..<Bands.count {
                dst[i] = ac_band_fitting(gain_soft_db: left[i].gainSoftDB, gain_loud_db: left[i].gainLoudDB,
                                         compression_ratio: left[i].compressionRatio)
            }
        }
        withUnsafeMutableBytes(of: &p.right) { raw in
            let dst = raw.bindMemory(to: ac_band_fitting.self)
            for i in 0..<Bands.count {
                dst[i] = ac_band_fitting(gain_soft_db: right[i].gainSoftDB, gain_loud_db: right[i].gainLoudDB,
                                         compression_ratio: right[i].compressionRatio)
            }
        }
        p.mpo_dbfs = mpoDBFS.isNaN ? -3 : min(max(mpoDBFS, -20), -1)
        p.output_gain_db = outputGainDB.isNaN ? 0 : min(max(outputGainDB, -48), 12)
        p.attack_ms = mode.attackMS
        p.release_ms = mode.releaseMS
        p.limiter_enabled = limiter ? 1 : 0
        p.input_gain_db = inputGainDB.isNaN ? 0 : min(max(inputGainDB, -30), 12)
        p.auto_mode = mode == .auto ? 1 : 0
        p.active = active ? 1 : 0
        return p
    }

    private static func clampStrength(_ s: Double) -> Double {
        if s.isNaN { return 0 }
        return min(max(s, 0), 1)
    }
}
