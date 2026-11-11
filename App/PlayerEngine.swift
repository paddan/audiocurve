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
        // NS_REFINED_FOR_SWIFT throwing variant (the old installTap is deprecated in macOS 27).
        try mixer.__installTap(onBus: 0, bufferSize: 1024, format: nil, error: (), block: Self.makeTap(peakBox))
        try connect()
        timer = Timer.scheduledTimer(withTimeInterval: 0.05, repeats: true) { [weak self] _ in
            MainActor.assumeIsolated {
                guard let self else { return }
                let p = self.peakBox.get()
                self.outputPeakDB = p > 0 ? max(-120, 20 * log10(p)) : -120
            }
        }
    }

    nonisolated private static func makeTap(_ box: PeakBox) -> AVAudioNodeTapBlock {
        { buffer, _ in
            var peak: Float = 0
            if let data = buffer.floatChannelData {
                for c in 0..<Int(buffer.format.channelCount) {
                    for i in 0..<Int(buffer.frameLength) { peak = max(peak, abs(data[c][i])) }
                }
            }
            box.set(peak)
        }
    }

    private func connect() throws {
        guard let effect else { return }
        let format = file?.processingFormat ?? AVAudioFormat(standardFormatWithSampleRate: 48000, channels: 2)
        engine.disconnectNodeOutput(player)
        engine.disconnectNodeOutput(effect)
        try engine.connectNode(player, to: effect, format: format)
        try engine.connectNode(effect, to: engine.mainMixerNode, format: format)
    }

    func open(file url: URL) throws {
        stop()
        file = try AVAudioFile(forReading: url)
        try connect()
    }

    func play() {
        guard let file else { return }
        if !engine.isRunning {
            engine.prepare()
            try? engine.start()
        }
        player.scheduleFile(file, at: nil) { }
        try? player.playAudio()
    }

    func stop() {
        player.stop()
        file?.framePosition = 0
    }

    var bypass: Bool {
        get { effect?.withAUAudioUnit { $0.shouldBypassEffect } ?? false }
        set { effect?.withAUAudioUnit { $0.shouldBypassEffect = newValue } }
    }

    func pluginViewController() async -> NSViewController? {
        await withCheckedContinuation { (cont: CheckedContinuation<NSViewController?, Never>) in
            guard let effect else { cont.resume(returning: nil); return }
            effect.withAUAudioUnit { au in
                au.requestViewController { vc in cont.resume(returning: vc) }
            }
        }
    }
}

final class PeakBox: @unchecked Sendable {
    private let lock = NSLock()
    private var value: Float = 0
    func set(_ v: Float) { lock.lock(); value = max(v, value * 0.9); lock.unlock() }
    func get() -> Float { lock.lock(); defer { lock.unlock() }; return value }
}
