import Foundation

public enum Transducer: String, Codable, Sendable { case inEar, overEar }

/// Maps digital level (dBFS) to hearing level (dB HL) for the hearing test.
public struct TestCalibration: Codable, Equatable, Sendable {
    public enum Kind: String, Codable, Sendable { case nominal }

    public var transducer: Transducer
    /// SPL produced by a full-scale (0 dBFS) sine.
    public var fullScaleSPL: Double
    public var kind: Kind

    public init(transducer: Transducer, fullScaleSPL: Double = 100, kind: Kind = .nominal) {
        self.transducer = transducer
        self.fullScaleSPL = fullScaleSPL
        self.kind = kind
    }

    public static func nominal(_ t: Transducer) -> TestCalibration {
        TestCalibration(transducer: t)
    }

    /// Reference equivalent threshold SPL (dB) at the standard audiogram frequencies.
    /// In-ear: ISO 389-2 (insert earphones, occluded-ear simulator, IEC 60318-4).
    /// Over-ear: ISO 389-8 (circumaural earphones, e.g. HDA 200, IEC 60318-2 coupler).
    private static let retsplInEar: [Double: Double] = [
        125: 26.0, 250: 14.0, 500: 5.5, 750: 2.0, 1000: 0.0, 1500: 2.0,
        2000: 3.0, 3000: 3.5, 4000: 5.5, 6000: 2.0, 8000: 0.0,
    ]
    private static let retsplOverEar: [Double: Double] = [
        125: 30.5, 250: 18.0, 500: 11.0, 750: 6.0, 1000: 5.5, 1500: 5.5,
        2000: 4.5, 3000: 2.5, 4000: 9.5, 6000: 17.0, 8000: 17.5,
    ]

    public static func retspl(_ t: Transducer, _ frequencyHz: Double) -> Double {
        let table = t == .inEar ? retsplInEar : retsplOverEar
        guard let v = table[frequencyHz] else {
            preconditionFailure("No RETSPL for \(frequencyHz) Hz")
        }
        return v
    }

    /// Maximum test tone level, never exceeded.
    public static func capDBFS(_ frequencyHz: Double) -> Double {
        switch frequencyHz {
        case ..<600: return -1
        case ..<1600: return -3
        default: return -5
        }
    }

    public func hl(fromDBFS dBFS: Double, at f: Double) -> Double {
        dBFS + fullScaleSPL - Self.retspl(transducer, f)
    }

    public func dBFS(fromHL hl: Double, at f: Double) -> Double {
        hl - fullScaleSPL + Self.retspl(transducer, f)
    }

    /// Highest HL (multiple of 5) reachable without exceeding the dBFS cap.
    public func maxHL(at f: Double) -> Double {
        (hl(fromDBFS: Self.capDBFS(f), at: f) / 5).rounded(.down) * 5
    }

    public static func roundedHL(_ hl: Double) -> Double {
        min(120, max(-10, (hl / 5).rounded() * 5))
    }

    public static func defaultTransducer(deviceName: String) -> Transducer {
        deviceName.localizedCaseInsensitiveContains("AirPods") ? .inEar : .overEar
    }
}
