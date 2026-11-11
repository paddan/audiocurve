import AVFoundation
import AppKit
import CoreAudioKit

@MainActor
final class PlayerEngine {
    static let description = AudioComponentDescription(
        componentType: kAudioUnitType_Effect,
        componentSubType: 0x68636D70, // 'hcmp'
        componentManufacturer: 0x4C6E6473, // 'Lnds'
        componentFlags: 0, componentFlagsMask: 0)

    private let engine = AVAudioEngine()
    private let player = AVAudioPlayerNode()
    private var effect: AVAudioUnit?
    private var file: AVAudioFile?
    private var peakLinear: Float = 0
    private let peakBox = PeakBox()
    private var timer: Timer?
    private(set) var outputPeakDB: Float = -120

    func loadPlugin() async throws {
        let unit = try await AVAudioUnit.instantiate(with: Self.description, options: .loadOutOfProcess)
        effect = unit
        engine.attach(player)
        engine.attach(unit)
        let mixer = engine.mainMixerNode
        mixer.installTap(onBus: 0, bufferSize: 1024, format: nil) { [peakBox] buffer, _ in
            var peak: Float = 0
            if let data = buffer.floatChannelData {
                for c in 0..<Int(buffer.format.channelCount) {
                    for i in 0..<Int(buffer.frameLength) { peak = max(peak, abs(data[c][i])) }
                }
            }
            peakBox.set(peak)
        }
        connect()
        timer = Timer.scheduledTimer(withTimeInterval: 0.05, repeats: true) { [weak self] _ in
            MainActor.assumeIsolated {
                guard let self else { return }
                let p = self.peakBox.get()
                self.outputPeakDB = p > 0 ? max(-120, 20 * log10(p)) : -120
            }
        }
    }

    private func connect() {
        guard let effect else { return }
        let format = file?.processingFormat ?? AVAudioFormat(standardFormatWithSampleRate: 48000, channels: 2)
        engine.disconnectNodeOutput(player)
        engine.disconnectNodeOutput(effect)
        engine.connect(player, to: effect, format: format)
        engine.connect(effect, to: engine.mainMixerNode, format: format)
    }

    func open(file url: URL) throws {
        stop()
        file = try AVAudioFile(forReading: url)
        connect()
    }

    func play() {
        guard let file else { return }
        if !engine.isRunning {
            engine.prepare()
            try? engine.start()
        }
        player.scheduleFile(file, at: nil) { }
        player.play()
    }

    func stop() {
        player.stop()
        file?.framePosition = 0
    }

    var bypass: Bool {
        get { effect?.auAudioUnit.shouldBypassEffect ?? false }
        set { effect?.auAudioUnit.shouldBypassEffect = newValue }
    }

    func pluginViewController() async -> NSViewController? {
        await withCheckedContinuation { (cont: CheckedContinuation<NSViewController?, Never>) in
            guard let au = effect?.auAudioUnit else { cont.resume(returning: nil); return }
            au.requestViewController { vc in cont.resume(returning: vc) }
        }
    }
}

final class PeakBox: @unchecked Sendable {
    private let lock = NSLock()
    private var value: Float = 0
    func set(_ v: Float) { lock.lock(); value = max(v, value * 0.9); lock.unlock() }
    func get() -> Float { lock.lock(); defer { lock.unlock() }; return value }
}
