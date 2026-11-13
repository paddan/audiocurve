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
    /// `auto`: the kernel glides between the Speech and Music fitting and time constants by its classifier.
    case speech = 0, music = 1, auto = 2
}

/// Which prescription a fitting set follows (spec §4.2).
public enum Prescription: Sendable {
    /// NAL-inspired fitting for intelligibility.
    case speech
    /// Near-linear fitting for music: gentle compression, little treble boost.
    case music
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

    /// Per-band fitting for one ear. `strength` and `tone` are clamped to 0...1; NaN counts as 0.
    public static func bands(for p: HearingProfile, ear: Ear, strength: Double, tone: Double = 0,
                             prescription: Prescription) -> [BandFitting] {
        let s = clampStrength(strength)
        let tn = clampStrength(tone)
        let audiogram = p.audiogram(ear)
        let trim = p.trim(ear)
        let maxRatio = prescription == .speech ? 3.0 : 2.0
        return (0..<Bands.count).map { i in
            let f = Bands.centers[i]
            let hl = interpolatedHL(audiogram, at: f)
            // Effective loss: above 60 dB HL only half of the extra loss is prescribed.
            let hle = hl <= 60 ? hl : 60 + 0.5 * (hl - 60)
            var soft: Double, loud: Double
            switch prescription {
            case .speech:
                let corr: Double
                switch f {
                case ...250: corr = 10
                case 500: corr = 5
                default: corr = 0
                }
                soft = 0.6 * hle - corr
                loud = 0.25 * hle - corr
            case .music:
                soft = 0.45 * hle
                loud = (f <= 2000 ? 0.3 : 0.2) * hle
            }
            // Tone: more bass and low mids, less loud-level treble.
            let boost: Double
            switch f {
            case ...250: boost = 5
            case 500: boost = 3
            case 750, 1000: boost = 1
            default: boost = 0
            }
            soft += boost * tn
            loud += boost * tn
            if f > 2000 { loud -= 6 * tn }
            let t = i < trim.count ? trim[i] : 0
            let gSoft = (max(0, soft) + t) * s
            let gLoud = (max(0, loud) + t) * s
            return BandFitting(gainSoftDB: gSoft, gainLoudDB: gLoud,
                               compressionRatio: ratio(soft: gSoft, loud: gLoud, max: maxRatio))
        }
    }

    /// Compression ratio from the soft-to-loud gain difference over a 60 dB input range.
    private static func ratio(soft: Double, loud: Double, max maxRatio: Double) -> Double {
        let den = 60 - (soft - loud)
        return den <= 0 ? maxRatio : min(max(60 / den, 1), maxRatio)
    }

    /// Linear blend of two fittings for display (weight 1 = `speech`, 0 = `music`). The ratio is
    /// recomputed from the blended gains and not clamped.
    public static func blend(_ speech: [BandFitting], _ music: [BandFitting], weight: Double) -> [BandFitting] {
        let w = weight.isNaN ? 0 : min(max(weight, 0), 1)
        return zip(speech, music).map { s, m in
            let soft = w * s.gainSoftDB + (1 - w) * m.gainSoftDB
            let loud = w * s.gainLoudDB + (1 - w) * m.gainLoudDB
            let den = 60 - (soft - loud)
            return BandFitting(gainSoftDB: soft, gainLoudDB: loud, compressionRatio: den <= 0 ? 10 : max(60 / den, 1))
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
    public static func params(profile: HearingProfile?, strength: Double, tone: Double = 0, mpoDBFS: Double,
                              outputGainDB: Double, mode: CompressionMode,
                              compression: Bool = true, limiter: Bool = true,
                              inputGainDB: Double = 0, active: Bool = true) -> ac_params {
        var p = ac_params()
        let s = clampStrength(strength)
        let unity = BandFitting(gainSoftDB: 0, gainLoudDB: 0, compressionRatio: 1)
        func linear(_ b: [BandFitting]) -> [BandFitting] {
            compression ? b : b.map { BandFitting(gainSoftDB: $0.gainLoudDB, gainLoudDB: $0.gainLoudDB, compressionRatio: 1) }
        }
        func set(_ ear: Ear, _ rx: Prescription) -> [BandFitting] {
            linear(profile.map { bands(for: $0, ear: ear, strength: s, tone: tone, prescription: rx) }
                   ?? Array(repeating: unity, count: Bands.count))
        }
        func fill(_ set: UnsafeMutableRawBufferPointer, _ b: [BandFitting]) {
            let dst = set.bindMemory(to: ac_band_fitting.self)
            for i in 0..<Bands.count {
                dst[i] = ac_band_fitting(gain_soft_db: b[i].gainSoftDB, gain_loud_db: b[i].gainLoudDB,
                                         compression_ratio: b[i].compressionRatio)
            }
        }
        withUnsafeMutableBytes(of: &p.left) { fill($0, set(.left, .speech)) }
        withUnsafeMutableBytes(of: &p.right) { fill($0, set(.right, .speech)) }
        withUnsafeMutableBytes(of: &p.music_left) { fill($0, set(.left, .music)) }
        withUnsafeMutableBytes(of: &p.music_right) { fill($0, set(.right, .music)) }
        p.mpo_dbfs = mpoDBFS.isNaN ? -3 : min(max(mpoDBFS, -20), -1)
        p.output_gain_db = outputGainDB.isNaN ? 0 : min(max(outputGainDB, -48), 12)
        p.speech_weight = mode == .speech ? 1 : 0
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
