import Foundation
import Testing
@testable import AudioCurveCore

@Suite struct ToneSynthTests {
    static let fs = 48_000.0

    /// Renders `seconds` of tone in blocks of 512 frames.
    static func renderAll(ear: Ear, f: Double, level: Double?, seconds: Double = 1.1)
        -> (left: [Float], right: [Float], finished: Bool)
    {
        var synth = ToneSynth(sampleRate: fs)
        synth.start(ear: ear, frequencyHz: f, levelDBFS: level)
        let total = Int(seconds * fs)
        var l = [Float](repeating: 1, count: total)
        var r = [Float](repeating: 1, count: total)
        var pos = 0
        while pos < total {
            let n = min(512, total - pos)
            l.withUnsafeMutableBufferPointer { lp in
                r.withUnsafeMutableBufferPointer { rp in
                    synth.render(left: lp.baseAddress! + pos, right: rp.baseAddress! + pos, frames: n)
                }
            }
            pos += n
        }
        return (l, r, synth.isFinished)
    }

    static func peak(_ x: ArraySlice<Float>) -> Float { x.map { abs($0) }.max() ?? 0 }
    static func range(_ a: Double, _ b: Double) -> Range<Int> { Int(a * fs)..<Int(b * fs) }

    @Test func clampsToCapEvenIfAskedLouder() {
        let out = Self.renderAll(ear: .left, f: 4000, level: 0)
        let cap = Float(pow(10.0, -5.0 / 20) + 1e-6)
        #expect(Self.peak(out.left[...]) <= cap)
        #expect(Self.peak(out.left[...]) > 0.5)
    }

    @Test func infiniteLevelIsSilentAndFinite() {
        for level in [Double.infinity, -Double.infinity, Double.nan] {
            let out = Self.renderAll(ear: .left, f: 1000, level: level)
            #expect(out.left.allSatisfy { $0.isFinite })
            #expect(Self.peak(out.left[...]) == 0)
            #expect(Self.peak(out.right[...]) == 0)
        }
    }

    /// A request above the cap plays a sine at the cap, not a hard-clipped square wave.
    @Test func overCapRequestIsAScaledSineNotClipped() {
        for level in [0.0, 20.0, 200.0] {
            let out = Self.renderAll(ear: .left, f: 4000, level: level)
            let capF = Float(pow(10.0, -5.0 / 20))
            let plateau = out.left[Self.range(0.05, 0.15)]
            #expect(Self.peak(plateau) <= capF + 1e-6)
            #expect(Self.peak(plateau) > capF * 0.99)
            // A sine's RMS is peak/√2; a clipped (squarer) wave's is higher.
            let rms = sqrt(plateau.reduce(0.0) { $0 + Double($1) * Double($1) } / Double(plateau.count))
            #expect(abs(rms / Double(Self.peak(plateau)) - 0.5.squareRoot()) < 0.005)
        }
    }

    @Test(arguments: [(250.0, -1.0), (1000.0, -3.0), (4000.0, -5.0)])
    func capsPerFrequency(f: Double, capDB: Double) {
        let out = Self.renderAll(ear: .right, f: f, level: 0)
        let cap = Float(pow(10.0, capDB / 20))
        let p = Self.peak(out.right[...])
        #expect(p <= cap + 1e-6)
        #expect(p > cap * 0.99)
    }

    @Test func levelMatchesRequest() {
        let out = Self.renderAll(ear: .left, f: 1000, level: -40)
        let p = Double(Self.peak(out.left[Self.range(0.05, 0.15)]))
        let db = 20 * log10(p) - (-40)
        #expect(abs(db) < 0.1)
    }

    @Test func onlyChosenEar() {
        let out = Self.renderAll(ear: .left, f: 1000, level: -20)
        #expect(Self.peak(out.right[...]) == 0)
        #expect(Self.peak(out.left[...]) > 0)
        let out2 = Self.renderAll(ear: .right, f: 1000, level: -20)
        #expect(Self.peak(out2.left[...]) == 0)
        #expect(Self.peak(out2.right[...]) > 0)
    }

    @Test func rampsStartAndEndAtZero() {
        let out = Self.renderAll(ear: .left, f: 1000, level: -20)
        #expect(abs(out.left[0]) < 1e-4)
        #expect(abs(out.left[Int(0.2 * Self.fs) - 1]) < 1e-3)
        // Rises over 20 ms: early part is quieter than the plateau.
        let early = Self.peak(out.left[0..<Int(0.002 * Self.fs)])
        let plateau = Self.peak(out.left[Self.range(0.05, 0.15)])
        #expect(early < plateau * 0.1)
        let nearFull = Self.peak(out.left[Self.range(0.018, 0.022)])
        #expect(nearFull > plateau * 0.9)
    }

    @Test func threePulsesThenSilence() {
        let out = Self.renderAll(ear: .left, f: 1000, level: -20)
        for (a, b) in [(0.0, 0.2), (0.4, 0.6), (0.8, 1.0)] {
            #expect(Self.peak(out.left[Self.range(a + 0.03, b - 0.03)]) > 0.05)
        }
        for (a, b) in [(0.2, 0.4), (0.6, 0.8), (1.0, 1.1)] {
            #expect(Self.peak(out.left[Self.range(a, b)]) < 1e-6)
        }
        #expect(out.finished)
    }

    @Test func catchTrialIsSilent() {
        var synth = ToneSynth(sampleRate: Self.fs)
        synth.start(ear: .right, frequencyHz: 1000, levelDBFS: nil)
        let out = Self.renderAll(ear: .right, f: 1000, level: nil)
        #expect(Self.peak(out.left[...]) == 0)
        #expect(Self.peak(out.right[...]) == 0)
        #expect(out.finished)
        // Still lasts 1.0 s: not finished halfway.
        var l = [Float](repeating: 0, count: 24_000), r = l
        l.withUnsafeMutableBufferPointer { lp in
            r.withUnsafeMutableBufferPointer { rp in
                synth.render(left: lp.baseAddress!, right: rp.baseAddress!, frames: 24_000)
            }
        }
        #expect(!synth.isFinished)
    }
}
