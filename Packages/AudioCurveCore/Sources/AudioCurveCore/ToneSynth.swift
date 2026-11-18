import Foundation

/// Renders the hearing-test tone: three 200 ms pulses separated by 200 ms gaps,
/// phase-continuous, with raised-cosine ramps and a final hard level clamp.
/// Realtime-safe: `render` neither allocates nor locks.
public struct ToneSynth: Sendable {
    public static let pulseOn = 0.2
    public static let pulseOff = 0.2
    public static let pulses = 3
    public static let ramp = 0.02
    public static var duration: Double { 1.0 }

    private let sampleRate: Double
    private var ear: Ear = .left
    private var amplitude = 0.0      // linear peak; 0 = silent catch tone
    private var capLinear = 0.0
    private var phase = 0.0
    private var phaseStep = 0.0
    private var position = 0         // samples rendered since start
    private var active = false

    public init(sampleRate: Double) {
        self.sampleRate = sampleRate
    }

    public var isFinished: Bool {
        !active || position >= Int(Self.duration * sampleRate)
    }

    public mutating func start(ear: Ear, frequencyHz: Double, levelDBFS: Double?) {
        self.ear = ear
        capLinear = pow(10, TestCalibration.capDBFS(frequencyHz) / 20)
        // Never above the cap (a louder request plays a sine at the cap, not a clipped one);
        // a non-finite level is silent.
        if let l = levelDBFS, l.isFinite {
            amplitude = min(pow(10, l / 20), capLinear)
        } else {
            amplitude = 0
        }
        phase = 0
        phaseStep = 2 * Double.pi * frequencyHz / sampleRate
        position = 0
        active = true
    }

    public mutating func render(left: UnsafeMutablePointer<Float>,
                                right: UnsafeMutablePointer<Float>,
                                frames: Int) {
        let total = Int(Self.duration * sampleRate)
        let period = Self.pulseOn + Self.pulseOff
        let rampLen = Self.ramp
        for i in 0..<frames {
            var out = 0.0
            if active, position < total, amplitude > 0 {
                let t = Double(position) / sampleRate
                let k = Int(t / period)
                let local = t - Double(k) * period
                if k < Self.pulses, local < Self.pulseOn {
                    var env = 1.0
                    if local < rampLen {
                        env = 0.5 - 0.5 * cos(Double.pi * local / rampLen)
                    } else if local > Self.pulseOn - rampLen {
                        env = 0.5 - 0.5 * cos(Double.pi * (Self.pulseOn - local) / rampLen)
                    }
                    out = amplitude * env * sin(phase)
                }
                phase += phaseStep
                if phase >= 2 * Double.pi { phase -= 2 * Double.pi }
            }
            if active { position += 1 }
            // Hard clamp, applied after all gain.
            let clamped = Float(min(max(out, -capLinear), capLinear))
            left[i] = ear == .left ? clamped : 0
            right[i] = ear == .right ? clamped : 0
        }
    }
}
