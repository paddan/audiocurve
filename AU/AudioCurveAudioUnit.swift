import AVFoundation
import AudioToolbox
import CAudioCurveDSP

public final class AudioCurveAudioUnit: AUAudioUnit {
    private var inputBus: AUAudioUnitBus
    private var outputBus: AUAudioUnitBus
    private var inputBusArray: AUAudioUnitBusArray!
    private var outputBusArray: AUAudioUnitBusArray!
    private var kernel: OpaquePointer?
    private var scratch: UnsafeMutablePointer<Float>?
    private var scratchFrames: Int = 0
    private let maxChannels = 2

    public override init(componentDescription: AudioComponentDescription,
                         options: AudioComponentInstantiationOptions = []) throws {
        let format = AVAudioFormat(standardFormatWithSampleRate: 48000, channels: 2)!
        inputBus = try AUAudioUnitBus(format: format)
        outputBus = try AUAudioUnitBus(format: format)
        try super.init(componentDescription: componentDescription, options: options)
        inputBusArray = AUAudioUnitBusArray(audioUnit: self, busType: .input, busses: [inputBus])
        outputBusArray = AUAudioUnitBusArray(audioUnit: self, busType: .output, busses: [outputBus])
        maximumFramesToRender = 4096
    }

    deinit { releaseResources() }

    public override var inputBusses: AUAudioUnitBusArray { inputBusArray }
    public override var outputBusses: AUAudioUnitBusArray { outputBusArray }

    public override var latency: TimeInterval { 0 }

    public override var channelCapabilities: [NSNumber]? { [2, 2] }

    private func releaseResources() {
        if let k = kernel { ac_kernel_free(k); kernel = nil }
        scratch?.deallocate(); scratch = nil
        scratchFrames = 0
    }

    public override func allocateRenderResources() throws {
        let fmt = outputBus.format
        guard inputBus.format.sampleRate == fmt.sampleRate,
              fmt.channelCount == 2, fmt.commonFormat == .pcmFormatFloat32, !fmt.isInterleaved else {
            throw NSError(domain: NSOSStatusErrorDomain, code: Int(kAudioUnitErr_FormatNotSupported))
        }
        try super.allocateRenderResources()
        releaseResources()
        guard let k = ac_kernel_new(fmt.sampleRate, maximumFramesToRender) else {
            throw NSError(domain: NSOSStatusErrorDomain, code: Int(kAudioUnitErr_FailedInitialization))
        }
        kernel = k
        // Fallback storage for hosts that pass null mData in the output list.
        scratchFrames = Int(maximumFramesToRender)
        scratch = .allocate(capacity: scratchFrames * maxChannels)
        scratch?.initialize(repeating: 0, count: scratchFrames * maxChannels)
    }

    public override func deallocateRenderResources() {
        releaseResources()
        super.deallocateRenderResources()
    }

    public override var internalRenderBlock: AUInternalRenderBlock {
        let scratch = self.scratch
        let scratchFrames = self.scratchFrames
        return { _, timestamp, frameCount, _, outputData, _, pullInputBlock in
            guard let pull = pullInputBlock else { return kAudioUnitErr_NoConnection }
            let abl = UnsafeMutableAudioBufferListPointer(outputData)
            if let scratch, Int(frameCount) <= scratchFrames {
                for (i, buf) in abl.enumerated() where buf.mData == nil {
                    abl[i].mData = UnsafeMutableRawPointer(scratch + i * scratchFrames)
                    abl[i].mDataByteSize = frameCount * UInt32(MemoryLayout<Float>.size)
                }
            }
            var flags = AudioUnitRenderActionFlags()
            return pull(&flags, timestamp, frameCount, 0, outputData)
        }
    }
}
