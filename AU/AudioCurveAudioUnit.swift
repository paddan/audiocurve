import AVFoundation
import AudioToolbox
import CAudioCurveDSP

public final class AudioCurveAudioUnit: AUAudioUnit {
    private var inputBus: AUAudioUnitBus
    private var outputBus: AUAudioUnitBus
    private var inputBusArray: AUAudioUnitBusArray!
    private var outputBusArray: AUAudioUnitBusArray!
    /// Stable heap state read by the render block. The block captures only this pointer,
    /// so hosts may cache the block across allocate/deallocate cycles.
    private struct RenderState {
        var kernel: OpaquePointer?
        var maxFrames: UInt32 = 0
        var scratch: UnsafeMutablePointer<Float>
    }
    private static let maxSupportedFrames = 16384
    private static let maxChannels = 2
    private let state: UnsafeMutablePointer<RenderState>

    public override init(componentDescription: AudioComponentDescription,
                         options: AudioComponentInstantiationOptions = []) throws {
        let format = AVAudioFormat(standardFormatWithSampleRate: 48000, channels: 2)!
        let scratch = UnsafeMutablePointer<Float>.allocate(capacity: Self.maxSupportedFrames * Self.maxChannels)
        scratch.initialize(repeating: 0, count: Self.maxSupportedFrames * Self.maxChannels)
        state = .allocate(capacity: 1)
        state.initialize(to: RenderState(kernel: nil, maxFrames: 0, scratch: scratch))
        inputBus = try AUAudioUnitBus(format: format)
        outputBus = try AUAudioUnitBus(format: format)
        try super.init(componentDescription: componentDescription, options: options)
        inputBusArray = AUAudioUnitBusArray(audioUnit: self, busType: .input, busses: [inputBus])
        outputBusArray = AUAudioUnitBusArray(audioUnit: self, busType: .output, busses: [outputBus])
        maximumFramesToRender = 4096
    }

    deinit {
        releaseResources()
        state.pointee.scratch.deallocate()
        state.deallocate()
    }

    public override var inputBusses: AUAudioUnitBusArray { inputBusArray }
    public override var outputBusses: AUAudioUnitBusArray { outputBusArray }

    public override var latency: TimeInterval { 0 }

    public override var channelCapabilities: [NSNumber]? { [2, 2] }

    private func releaseResources() {
        if let k = state.pointee.kernel { ac_kernel_free(k); state.pointee.kernel = nil }
    }

    public override func allocateRenderResources() throws {
        let fmt = outputBus.format
        guard inputBus.format.sampleRate == fmt.sampleRate,
              fmt.channelCount == 2, fmt.commonFormat == .pcmFormatFloat32, !fmt.isInterleaved,
              Int(maximumFramesToRender) <= Self.maxSupportedFrames else {
            throw NSError(domain: NSOSStatusErrorDomain, code: Int(kAudioUnitErr_FormatNotSupported))
        }
        try super.allocateRenderResources()
        releaseResources()
        guard let k = ac_kernel_new(fmt.sampleRate, maximumFramesToRender) else {
            throw NSError(domain: NSOSStatusErrorDomain, code: Int(kAudioUnitErr_FailedInitialization))
        }
        state.pointee.kernel = k
        state.pointee.maxFrames = maximumFramesToRender
    }

    public override func deallocateRenderResources() {
        releaseResources()
        super.deallocateRenderResources()
    }

    public override var internalRenderBlock: AUInternalRenderBlock {
        let state = self.state
        let stride = Self.maxSupportedFrames
        return { _, timestamp, frameCount, _, outputData, _, pullInputBlock in
            guard frameCount <= state.pointee.maxFrames else { return kAudioUnitErr_TooManyFramesToProcess }
            guard let pull = pullInputBlock else { return kAudioUnitErr_NoConnection }
            let abl = UnsafeMutableAudioBufferListPointer(outputData)
            for i in 0..<abl.count where abl[i].mData == nil {
                // Host passed no buffer: render into the stable internal scratch (stereo max).
                abl[i].mData = UnsafeMutableRawPointer(state.pointee.scratch + min(i, 1) * stride)
                abl[i].mDataByteSize = frameCount * UInt32(MemoryLayout<Float>.size)
            }
            var flags = AudioUnitRenderActionFlags()
            return pull(&flags, timestamp, frameCount, 0, outputData)
        }
    }
}
