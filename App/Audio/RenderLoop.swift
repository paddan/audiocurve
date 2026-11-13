import AudioToolbox
import AVFoundation
import CoreAudio
import Synchronization

/// The IOProc on the aggregate device: tap input → plugin → output device.
/// The IO block does not allocate, lock or log; every buffer is allocated in `init`.
final class RenderLoop: @unchecked Sendable {
    private let device: AudioObjectID
    private let maxFrames: Int
    private let render: AURenderBlock
    private let onRenderFailure: @Sendable () -> Void
    private let io = DispatchQueue(label: "se.lindefors.audiocurve.route-io")
    private var procID: AudioDeviceIOProcID?

    // Preallocated buffers: plugin input and output, two non-interleaved channels each.
    private let inL, inR, outL, outR: UnsafeMutablePointer<Float>
    private let inList, outList: UnsafeMutableAudioBufferListPointer
    private var sampleTime: Float64 = 0
    /// Created once: a closure literal in the IO block would allocate on every cycle.
    private let pull: AURenderPullInputBlock
    private let failed = Atomic<Bool>(false)
    /// Host time of the last input buffer with a non-zero sample (0: none yet).
    private let lastSignal = Atomic<UInt64>(0)

    init(device: AudioObjectID, unit: AUAudioUnit, maxFrames: AUAudioFrameCount,
         onRenderFailure: @escaping @Sendable () -> Void) throws {
        self.device = device
        self.maxFrames = Int(maxFrames)
        self.render = unit.renderBlock
        self.onRenderFailure = onRenderFailure
        func buffer() -> UnsafeMutablePointer<Float> {
            let p = UnsafeMutablePointer<Float>.allocate(capacity: Int(maxFrames))
            p.initialize(repeating: 0, count: Int(maxFrames))
            return p
        }
        inL = buffer(); inR = buffer(); outL = buffer(); outR = buffer()
        inList = AudioBufferList.allocate(maximumBuffers: 2)
        outList = AudioBufferList.allocate(maximumBuffers: 2)
        let source = inList
        pull = { _, _, _, _, data in
            let dst = UnsafeMutableAudioBufferListPointer(data)
            for i in 0..<min(dst.count, 2) {
                if dst[i].mData == nil { dst[i].mData = source[i].mData }
                else { memcpy(dst[i].mData, source[i].mData, Int(source[i].mDataByteSize)) }
                dst[i].mDataByteSize = source[i].mDataByteSize
            }
            return noErr
        }

        var proc: AudioDeviceIOProcID?
        try check(AudioDeviceCreateIOProcIDWithBlock(&proc, device, nil) { [unowned self] _, input, _, output, outTime in
            self.cycle(input: input, output: output, time: outTime)
        }, "AudioDeviceCreateIOProcIDWithBlock")
        procID = proc
    }

    deinit {
        stop()
        for p in [inL, inR, outL, outR] { p.deallocate() }
        free(inList.unsafeMutablePointer)
        free(outList.unsafeMutablePointer)
    }

    /// Starts IO off the main thread: the first start blocks while the system asks for the
    /// recording permission. A start error is reported like a render failure.
    func start() {
        io.async { [self] in
            guard let procID else { return }
            if AudioDeviceStart(device, procID) != noErr { reportFailure() }
        }
    }

    /// Stops IO and destroys the IOProc. Waits for a pending start.
    func stop() {
        io.sync {
            guard let procID else { return }
            AudioDeviceStop(device, procID)
            AudioDeviceDestroyIOProcID(device, procID)
            self.procID = nil
        }
    }

    /// Seconds since the input last carried sound, or nil if it never has.
    var secondsSinceSignal: Double? {
        let t = lastSignal.load(ordering: .relaxed)
        guard t != 0 else { return nil }
        let now = AudioGetCurrentHostTime()
        return Double(AudioConvertHostTimeToNanos(now - min(t, now))) / 1e9
    }

    private func reportFailure() {
        if !failed.exchange(true, ordering: .relaxed) { onRenderFailure() }
    }

    private func cycle(input: UnsafePointer<AudioBufferList>, output: UnsafeMutablePointer<AudioBufferList>,
                       time: UnsafePointer<AudioTimeStamp>) {
        let out = UnsafeMutableAudioBufferListPointer(output)
        let frames = Self.frameCount(out)
        guard frames > 0, frames <= maxFrames, !failed.load(ordering: .relaxed) else { Self.zero(out); return }

        readInput(UnsafeMutableAudioBufferListPointer(UnsafeMutablePointer(mutating: input)), frames: frames)
        let bytes = UInt32(frames * MemoryLayout<Float>.size)
        Self.point(inList, inL, inR, bytes)
        Self.point(outList, outL, outR, bytes)

        var flags = AudioUnitRenderActionFlags()
        var ts = AudioTimeStamp()
        ts.mSampleTime = sampleTime
        ts.mHostTime = time.pointee.mHostTime
        ts.mFlags = [.sampleTimeValid, .hostTimeValid]
        sampleTime += Float64(frames)
        let status = render(&flags, &ts, AUAudioFrameCount(frames), 0, outList.unsafeMutablePointer, pull)
        guard status == noErr else { Self.zero(out); reportFailure(); return }
        writeOutput(out, frames: frames)
    }

    /// Copies the tap input into inL/inR (interleaved stereo, two mono buffers, or one mono buffer).
    private func readInput(_ input: UnsafeMutableAudioBufferListPointer, frames: Int) {
        var peak: Float = 0
        defer { if peak > 0 { lastSignal.store(AudioGetCurrentHostTime(), ordering: .relaxed) } }
        // A buffer shorter than the output cycle is never read past its end.
        let available = input.first.map { Int($0.mDataByteSize) / (MemoryLayout<Float>.size * max(Int($0.mNumberChannels), 1)) } ?? 0
        guard available >= frames, let first = input.first, let d0 = first.mData?.assumingMemoryBound(to: Float.self) else {
            inL.update(repeating: 0, count: frames); inR.update(repeating: 0, count: frames); return
        }
        if first.mNumberChannels >= 2 {
            let ch = Int(first.mNumberChannels)
            for i in 0..<frames { inL[i] = d0[i * ch]; inR[i] = d0[i * ch + 1]; peak = max(peak, abs(inL[i]), abs(inR[i])) }
        } else {
            let second = input.count > 1 && Int(input[1].mDataByteSize) >= frames * MemoryLayout<Float>.size
            let d1 = second ? input[1].mData?.assumingMemoryBound(to: Float.self) ?? d0 : d0
            for i in 0..<frames { inL[i] = d0[i]; inR[i] = d1[i]; peak = max(peak, abs(inL[i]), abs(inR[i])) }
        }
    }

    /// Writes outL/outR to the device buffers, whatever their layout; extra channels get silence.
    private func writeOutput(_ out: UnsafeMutableAudioBufferListPointer, frames: Int) {
        let l = outList[0].mData!.assumingMemoryBound(to: Float.self)
        let r = outList[1].mData!.assumingMemoryBound(to: Float.self)
        for (b, buf) in out.enumerated() {
            guard let d = buf.mData?.assumingMemoryBound(to: Float.self) else { continue }
            let ch = Int(buf.mNumberChannels)
            if ch >= 2 {
                for i in 0..<frames {
                    d[i * ch] = l[i]; d[i * ch + 1] = r[i]
                    for c in 2..<ch { d[i * ch + c] = 0 }
                }
            } else if out.count == 1 {
                for i in 0..<frames { d[i] = 0.5 * (l[i] + r[i]) }
            } else {
                let src: UnsafeMutablePointer<Float>? = b == 0 ? l : b == 1 ? r : nil
                if let src { d.update(from: src, count: frames) } else { d.update(repeating: 0, count: frames) }
            }
        }
    }

    private static func frameCount(_ out: UnsafeMutableAudioBufferListPointer) -> Int {
        guard let b = out.first, b.mNumberChannels > 0 else { return 0 }
        return Int(b.mDataByteSize) / (MemoryLayout<Float>.size * Int(b.mNumberChannels))
    }

    private static func point(_ list: UnsafeMutableAudioBufferListPointer, _ l: UnsafeMutablePointer<Float>,
                              _ r: UnsafeMutablePointer<Float>, _ bytes: UInt32) {
        list[0] = AudioBuffer(mNumberChannels: 1, mDataByteSize: bytes, mData: l)
        list[1] = AudioBuffer(mNumberChannels: 1, mDataByteSize: bytes, mData: r)
    }

    private static func zero(_ out: UnsafeMutableAudioBufferListPointer) {
        for b in out { if let d = b.mData { memset(d, 0, Int(b.mDataByteSize)) } }
    }
}
