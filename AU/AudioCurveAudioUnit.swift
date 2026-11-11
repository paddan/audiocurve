import AVFoundation
import AudioToolbox
import CAudioCurveDSP
import AudioCurveCore

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
        /// Host bypass request. Plain word written outside render, read once per render.
        var bypass: Bool = false
        /// Render-thread private: the previous render was bypassed.
        var wasBypassed: Bool = false
    }
    private static let maxSupportedFrames = 16384
    private static let maxChannels = 2
    private let state: UnsafeMutablePointer<RenderState>

    /// Guards `model`, `latestParams` and every control-side use of `state.kernel`
    /// (set_params / meters / free). Never taken on the render thread.
    private let lock = NSLock()
    private var model = ParamValues()
    private var _profile: HearingProfile?
    private var latestParams = ac_params()

    /// Called on the main queue after the profile changed (not called when the new value equals the current one).
    public var onProfileChange: (@MainActor (HearingProfile?) -> Void)?

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
        latestParams = FittingRule.params(profile: nil, strength: model.strength / 100, mpoDBFS: model.mpo,
                                          outputGainDB: model.outputGain, mode: .speech)
        installParameterTree()
    }

    private func installParameterTree() {
        let tree = Parameters.makeTree()
        tree.implementorValueObserver = { [weak self] param, value in
            guard let self, let a = ParamAddress(rawValue: param.address) else { return }
            self.lock.lock(); self.model[a] = Double(value); self.lock.unlock()
            self.recomputeOnMain()
        }
        tree.implementorValueProvider = { [weak self] param in
            guard let self, let a = ParamAddress(rawValue: param.address) else { return param.value }
            self.lock.lock(); defer { self.lock.unlock() }
            return AUValue(self.model[a])
        }
        parameterTree = tree
    }

    // MARK: Profile / parameters

    /// Thread-safe. Setting recomputes and pushes to the kernel on the main queue.
    public var profile: HearingProfile? {
        get { lock.lock(); defer { lock.unlock() }; return _profile }
        set {
            lock.lock()
            if _profile == newValue { lock.unlock(); return }
            _profile = newValue
            lock.unlock()
            Self.onMain { [self] in
                recompute()
                let cb = onProfileChange
                MainActor.assumeIsolated { cb?(newValue) }
            }
        }
    }

    /// Goes through the parameter tree so hosts see the change. Pass the UI's own observer token as
    /// `originator` to keep that observer from being notified about its own write.
    public func setParameter(_ a: ParamAddress, _ value: AUValue, originator: AUParameterObserverToken? = nil) {
        parameterTree?.parameter(withAddress: a.rawValue)?.setValue(value, originator: originator)
    }

    public func readMeters() -> ac_meters {
        var m = ac_meters()
        lock.lock(); defer { lock.unlock() }
        if let k = state.pointee.kernel { ac_kernel_read_meters(k, &m) } else {
            m.in_rms_db = (-120, -120); m.in_peak_db = (-120, -120)
            m.out_rms_db = (-120, -120); m.out_peak_db = (-120, -120)
        }
        return m
    }

    private struct MainBox: @unchecked Sendable { let f: () -> Void }
    private static func onMain(_ f: @escaping () -> Void) {
        if Thread.isMainThread { f() } else {
            let box = MainBox(f: f)
            DispatchQueue.main.async { box.f() }
        }
    }

    private func recomputeOnMain() { Self.onMain { [self] in recompute() } }

    /// Always runs on the main queue, so `ac_kernel_set_params` has a single writer thread.
    private func recompute() {
        lock.lock(); defer { lock.unlock() }
        let p = FittingRule.params(profile: _profile, strength: model.strength / 100, mpoDBFS: model.mpo,
                                   outputGainDB: model.outputGain,
                                   mode: CompressionMode(rawValue: model.mode) ?? .speech)
        latestParams = p
        if let k = state.pointee.kernel { withUnsafePointer(to: p) { ac_kernel_set_params(k, $0) } }
    }

    public override var fullState: [String: Any]? {
        get {
            var d = super.fullState ?? [:]
            d[PluginState.key] = PluginState(version: PluginState.currentVersion, profile: profile).encoded()
            return d
        }
        set {
            super.fullState = newValue
            let st = PluginState.decode(newValue?[PluginState.key] as? Data)
            profile = st.profile
        }
    }

    public override func shouldChange(to format: AVAudioFormat, for bus: AUAudioUnitBus) -> Bool {
        format.channelCount == 2 && format.commonFormat == .pcmFormatFloat32 && !format.isInterleaved
    }

    deinit {
        // No KVO notifications during deinit; free the kernel directly.
        if let k = state.pointee.kernel { ac_kernel_free(k) }
        state.pointee.kernel = nil
        state.pointee.scratch.deallocate()
        state.deallocate()
    }

    public override var inputBusses: AUAudioUnitBusArray { inputBusArray }
    public override var outputBusses: AUAudioUnitBusArray { outputBusArray }

    public override var latency: TimeInterval {
        lock.lock(); defer { lock.unlock() }
        guard let k = state.pointee.kernel else { return 0 }
        return Double(ac_kernel_latency_frames(k)) / outputBus.format.sampleRate
    }

    public override var shouldBypassEffect: Bool {
        get { state.pointee.bypass }
        set { state.pointee.bypass = newValue }
    }

    /// Clears envelopes and filter/delay state. Called by hosts outside render.
    public override func reset() {
        super.reset()
        lock.lock(); defer { lock.unlock() }
        if let k = state.pointee.kernel { ac_kernel_reset(k) }
    }

    public override var tailTime: TimeInterval { 0.1 }

    public override var channelCapabilities: [NSNumber]? { [2, 2] }

    private func releaseResources() {
        willChangeValue(forKey: "latency")
        lock.lock()
        state.pointee.maxFrames = 0          // cached render blocks now fail closed
        let k = state.pointee.kernel
        state.pointee.kernel = nil
        lock.unlock()
        if let k { ac_kernel_free(k) }
        didChangeValue(forKey: "latency")
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
        // allocate never runs concurrently with render, but may run on any thread; the lock
        // serialises it against recompute()'s set_params on the main queue.
        guard let k = ac_kernel_new(fmt.sampleRate, maximumFramesToRender) else {
            super.deallocateRenderResources()
            throw NSError(domain: NSOSStatusErrorDomain, code: Int(kAudioUnitErr_FailedInitialization))
        }
        willChangeValue(forKey: "latency")
        lock.lock()
        withUnsafePointer(to: latestParams) { ac_kernel_set_params(k, $0) }
        state.pointee.maxFrames = maximumFramesToRender
        state.pointee.kernel = k             // published only once fully set up
        lock.unlock()
        didChangeValue(forKey: "latency")
    }

    public override func deallocateRenderResources() {
        releaseResources()
        super.deallocateRenderResources()
    }

    public override var internalRenderBlock: AUInternalRenderBlock {
        let state = self.state
        let stride = Self.maxSupportedFrames
        return { _, timestamp, frameCount, _, outputData, _, pullInputBlock in
            let kernel = state.pointee.kernel
            guard frameCount <= state.pointee.maxFrames else { return kAudioUnitErr_TooManyFramesToProcess }
            guard let pull = pullInputBlock else { return kAudioUnitErr_NoConnection }
            let abl = UnsafeMutableAudioBufferListPointer(outputData)
            for i in 0..<abl.count where abl[i].mData == nil {
                // Host passed no buffer: render into the stable internal scratch (stereo max).
                abl[i].mData = UnsafeMutableRawPointer(state.pointee.scratch + min(i, 1) * stride)
                abl[i].mDataByteSize = frameCount * UInt32(MemoryLayout<Float>.size)
            }
            var flags = AudioUnitRenderActionFlags()
            let err = pull(&flags, timestamp, frameCount, 0, outputData)
            if state.pointee.bypass {
                state.pointee.wasBypassed = true
                return err
            }
            if state.pointee.wasBypassed {
                // Leaving bypass: drop stale envelope/delay state so it cannot burst.
                state.pointee.wasBypassed = false
                if let kernel { ac_kernel_reset(kernel) }
            }
            if err == noErr, let kernel, abl.count >= 2,
               let l = abl[0].mData, let r = abl[1].mData {
                ac_kernel_process(kernel, l.assumingMemoryBound(to: Float.self),
                                  r.assumingMemoryBound(to: Float.self), frameCount)
            }
            return err
        }
    }
}
