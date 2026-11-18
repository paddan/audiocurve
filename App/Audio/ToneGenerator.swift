import AudioCurveCore
import AudioToolbox
import AVFoundation
import CoreAudio
import Synchronization

/// Plays the hearing-test tones straight to one output device through AVAudioEngine.
///
/// Safety: the engine starts silent; every tone is limited to the per-frequency cap
/// (`TestCalibration.capDBFS`) here and again per sample in `ToneSynth`; a non-finite
/// level or a frequency outside the test range plays as silence; the render block
/// zeroes any non-finite sample. The render block never allocates, blocks or logs.
@MainActor
final class ToneGenerator {
    /// The test frequencies the cap table covers.
    static let frequencyRange = 125.0...8000.0
    /// Below this the top test frequencies sit at or near Nyquist and play as silence (a headset
    /// in call mode runs at 16 kHz); the test refuses to run.
    static let minimumSampleRate = 22_050.0

    private let engine = AVAudioEngine()
    private let device: AudioObjectID
    private let source: AVAudioSourceNode
    private let shared: Shared

    /// Binds the engine's output to the device with this UID. Nothing plays until `play`.
    init(deviceUID: String) throws {
        guard let device = HAL.device(uid: deviceUID) else {
            throw CoreAudioError(status: kAudioHardwareBadDeviceError, call: "ToneGenerator device \(deviceUID)")
        }
        self.device = device
        try engine.outputNode.withAudioUnit { (unit: borrowing AudioUnit?) throws in
            guard let unit = copy unit else {
                throw CoreAudioError(status: kAudioUnitErr_Uninitialized, call: "ToneGenerator output unit")
            }
            var id = device
            try check(AudioUnitSetProperty(unit, kAudioOutputUnitProperty_CurrentDevice, kAudioUnitScope_Global, 0,
                                           &id, UInt32(MemoryLayout<AudioObjectID>.size)),
                      "set kAudioOutputUnitProperty_CurrentDevice")
        }

        let sampleRate = engine.outputNode.outputFormat(forBus: 0).sampleRate
        guard sampleRate >= Self.minimumSampleRate, sampleRate.isFinite,
              let format = AVAudioFormat(standardFormatWithSampleRate: sampleRate, channels: 2) else {
            throw CoreAudioError(status: kAudioUnitErr_FormatNotSupported, call: "ToneGenerator format")
        }
        let shared = Shared(sampleRate: sampleRate)
        self.shared = shared
        source = AVAudioSourceNode(format: format, renderBlock: Self.renderBlock(shared))
        engine.attach(source)
        try engine.connectNode(source, to: engine.mainMixerNode, format: format)
        // No gain after the clamp: unity mixer volume, centred.
        engine.mainMixerNode.outputVolume = 1
        source.pan = 0
        source.volume = 1
    }

    /// Starts the engine, silent. Throws (with the engine stopped) if it does not run or does not
    /// play to the device it was bound to.
    func start() throws {
        engine.prepare()
        try engine.start()
        guard engine.isRunning else {
            throw CoreAudioError(status: kAudioHardwareNotRunningError, call: "ToneGenerator start")
        }
        guard boundDevice() == device else {
            stop()
            throw CoreAudioError(status: kAudioHardwareBadDeviceError, call: "ToneGenerator device binding")
        }
    }

    /// The output unit's current device, read back; nil if it cannot be read.
    private func boundDevice() -> AudioObjectID? {
        engine.outputNode.withAudioUnit { (unit: borrowing AudioUnit?) -> AudioObjectID? in
            guard let unit = copy unit else { return nil }
            var id = AudioObjectID(kAudioObjectUnknown)
            var size = UInt32(MemoryLayout<AudioObjectID>.size)
            let st = AudioUnitGetProperty(unit, kAudioOutputUnitProperty_CurrentDevice, kAudioUnitScope_Global, 0,
                                          &id, &size)
            return st == noErr ? id : nil
        }
    }

    var isRunning: Bool { engine.isRunning }

    /// Plays one pulsed tone (`ToneSynth.duration`). `levelDBFS` nil is a silent catch trial.
    /// A non-finite level or a frequency outside `frequencyRange` plays as silence.
    /// A tone already playing is replaced. Ignored while the engine is not running.
    func play(ear: Ear, frequencyHz: Double, levelDBFS: Double?) {
        guard engine.isRunning else {
            NSLog("AudioCurve: tone requested while the generator is not running")
            return
        }
        var level = levelDBFS
        if let l = level, !l.isFinite {
            NSLog("AudioCurve: non-finite tone level, playing silence")
            level = nil
        }
        var frequency = frequencyHz
        if !frequency.isFinite || !Self.frequencyRange.contains(frequency) {
            NSLog("AudioCurve: tone frequency %f out of range, playing silence", frequencyHz)
            frequency = 1000
            level = nil
        }
        // First guard: never ask the synth for more than the cap (the sample clamp is the second).
        level = level.map { min($0, TestCalibration.capDBFS(frequency)) }
        shared.request(.tone(ear: ear, frequencyHz: frequency, levelDBFS: level))
    }

    /// True from `play` until the tone (or silent catch trial) has finished rendering.
    var isPlaying: Bool { engine.isRunning && shared.isPlaying }

    /// Silences any tone and stops the engine. `start` may be called again.
    func stop() {
        shared.request(.silence)
        engine.stop()
        shared.markFinished()
    }

    // MARK: - Render side

    /// Created outside the main actor so the block is not main-actor isolated.
    nonisolated private static func renderBlock(_ shared: Shared) -> AVAudioSourceNodeRenderBlock {
        { _, _, frameCount, data in
            let list = UnsafeMutableAudioBufferListPointer(data)
            let frames = Int(frameCount)
            guard list.count >= 2,
                  let l = list[0].mData?.assumingMemoryBound(to: Float.self),
                  let r = list[1].mData?.assumingMemoryBound(to: Float.self),
                  Int(list[0].mDataByteSize) >= frames * MemoryLayout<Float>.size,
                  Int(list[1].mDataByteSize) >= frames * MemoryLayout<Float>.size else {
                for b in list { if let d = b.mData { memset(d, 0, Int(b.mDataByteSize)) } }
                return noErr
            }
            shared.render(left: l, right: r, frames: frames)
            return noErr
        }
    }

    /// State shared by the main thread and the render thread.
    private final class Shared: @unchecked Sendable {
        enum Command: Sendable {
            case tone(ear: Ear, frequencyHz: Double, levelDBFS: Double?)
            case silence
        }
        private struct Request: Sendable {
            var command: Command
            var generation: UInt64
        }

        /// Single-slot handoff; the render thread only try-locks it.
        private let slot = Mutex<Request?>(nil)
        /// Generation of the latest request, and of the latest one that finished rendering.
        private let requested = Atomic<UInt64>(0)
        private let finished = Atomic<UInt64>(0)

        // Render thread only (after init).
        private var synth: ToneSynth
        private var current: UInt64 = 0
        private let sampleRate: Double

        init(sampleRate: Double) {
            self.sampleRate = sampleRate
            synth = ToneSynth(sampleRate: sampleRate)
        }

        var isPlaying: Bool { requested.load(ordering: .acquiring) != finished.load(ordering: .acquiring) }

        /// Main thread.
        func request(_ command: Command) {
            slot.withLock { s in
                let g = requested.add(1, ordering: .releasing).newValue
                s = Request(command: command, generation: g)
            }
        }

        /// Main thread, after the engine has stopped: nothing is playing any more.
        func markFinished() {
            finished.store(requested.load(ordering: .acquiring), ordering: .releasing)
        }

        /// Render thread. Never blocks or allocates.
        func render(left: UnsafeMutablePointer<Float>, right: UnsafeMutablePointer<Float>, frames: Int) {
            if let r = slot.withLockIfAvailable({ s in
                let r = s
                s = nil
                return r
            }) ?? nil {
                current = r.generation
                switch r.command {
                case let .tone(ear, f, level): synth.start(ear: ear, frequencyHz: f, levelDBFS: level)
                case .silence: synth = ToneSynth(sampleRate: sampleRate)
                }
            }
            synth.render(left: left, right: right, frames: frames)
            // Last guard: never pass on a non-finite sample.
            for i in 0..<frames {
                if !left[i].isFinite { left[i] = 0 }
                if !right[i].isFinite { right[i] = 0 }
            }
            if synth.isFinished, finished.load(ordering: .relaxed) != current {
                finished.store(current, ordering: .releasing)
            }
        }
    }
}
