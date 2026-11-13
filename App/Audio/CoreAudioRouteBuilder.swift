import AudioCurveCore
import AVFoundation
import CoreAudio

/// Builds the real route: process tap → private aggregate device → IOProc through the plugin.
@MainActor
final class CoreAudioRouteBuilder: RouteBuilding {
    static let minimumBufferFrames: UInt32 = 256

    private let unit: () -> AUAudioUnit?
    private let onRenderFailure: @Sendable (UUID) -> Void
    private let onRouteInvalid: @MainActor () -> Void
    /// The running loop, for the silence-based permission check.
    private(set) var loop: RenderLoop?

    /// `onRenderFailure` gets the failing route's ID. `onRouteInvalid` is called when the output's
    /// sample rate changes, the output or the aggregate device dies, or coreaudiod restarts.
    init(unit: @escaping () -> AUAudioUnit?, onRenderFailure: @escaping @Sendable (UUID) -> Void,
         onRouteInvalid: @escaping @MainActor () -> Void) {
        self.unit = unit
        self.onRenderFailure = onRenderFailure
        self.onRouteInvalid = onRouteInvalid
    }

    func build(deviceUID: String) throws -> BuiltRoute {
        guard let unit = unit() else { throw CoreAudioError(status: -1, call: "plugin not loaded") }
        guard let output = HAL.device(uid: deviceUID) else { throw CoreAudioError(status: -1, call: "output \(deviceUID) not found") }
        let tap = try ProcessTap(excluding: [getpid()])
        let aggregate: AggregateDevice
        do { aggregate = try AggregateDevice(outputUID: deviceUID, tapUID: tap.uid) } catch { tap.destroy(); throw error }
        do {
            let frames = aggregate.setBufferFrames(atLeast: Self.minimumBufferFrames)
            try prepare(unit, sampleRate: aggregate.sampleRate, frames: frames)
            let id = UUID()
            let report = onRenderFailure
            let loop = try RenderLoop(device: aggregate.id, unit: unit, maxFrames: unit.maximumFramesToRender,
                                      onRenderFailure: { report(id) })
            let removers = [
                listen(output, kAudioDevicePropertyNominalSampleRate),
                listen(output, kAudioDevicePropertyDeviceIsAlive),
                listen(aggregate.id, kAudioDevicePropertyDeviceIsAlive),
                listen(AudioObjectID(kAudioObjectSystemObject), kAudioHardwarePropertyServiceRestarted),
            ]
            loop.start()
            self.loop = loop
            return BuiltRoute(
                id: id,
                stopIO: { [weak self] in loop.stop(); self?.loop = nil },
                destroyAggregate: { removers.forEach { $0() }; aggregate.destroy() },
                destroyTap: { tap.destroy() })
        } catch {
            aggregate.destroy()
            tap.destroy()
            throw error
        }
    }

    /// Stereo float32 non-interleaved at the device rate; resources re-allocated when the rate changed.
    private func prepare(_ unit: AUAudioUnit, sampleRate: Double, frames: UInt32) throws {
        guard sampleRate > 0, let format = AVAudioFormat(standardFormatWithSampleRate: sampleRate, channels: 2) else {
            throw CoreAudioError(status: -1, call: "output sample rate")
        }
        let sameFormat = unit.inputBusses[0].format == format && unit.outputBusses[0].format == format
        if unit.renderResourcesAllocated && sameFormat && unit.maximumFramesToRender >= frames { return }
        if unit.renderResourcesAllocated { unit.deallocateRenderResources() }
        try unit.inputBusses[0].setFormat(format)
        try unit.outputBusses[0].setFormat(format)
        unit.maximumFramesToRender = max(unit.maximumFramesToRender, frames, 4096)
        try unit.allocateRenderResources()
    }

    /// Calls `onRouteInvalid` when `selector` changes on `object`. Returns the remover.
    private func listen(_ object: AudioObjectID, _ selector: AudioObjectPropertySelector) -> () -> Void {
        var address = HAL.address(selector)
        let device = object
        let callback = onRouteInvalid
        let block: AudioObjectPropertyListenerBlock = { _, _ in MainActor.assumeIsolated { callback() } }
        guard AudioObjectAddPropertyListenerBlock(device, &address, .main, block) == noErr else { return {} }
        return {
            var a = address
            AudioObjectRemovePropertyListenerBlock(device, &a, .main, block)
        }
    }
}
