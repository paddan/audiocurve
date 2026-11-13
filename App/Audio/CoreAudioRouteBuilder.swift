import AudioCurveCore
import AVFoundation
import CoreAudio

/// Builds the real route: process tap → private aggregate device → IOProc through the plugin.
@MainActor
final class CoreAudioRouteBuilder: RouteBuilding {
    static let minimumBufferFrames: UInt32 = 256

    private let unit: () -> AUAudioUnit?
    private let onRenderFailure: @Sendable () -> Void
    private let onSampleRateChange: @MainActor () -> Void
    /// The running loop, for the silence-based permission check.
    private(set) var loop: RenderLoop?

    init(unit: @escaping () -> AUAudioUnit?, onRenderFailure: @escaping @Sendable () -> Void,
         onSampleRateChange: @escaping @MainActor () -> Void) {
        self.unit = unit
        self.onRenderFailure = onRenderFailure
        self.onSampleRateChange = onSampleRateChange
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
            let loop = try RenderLoop(device: aggregate.id, unit: unit,
                                      maxFrames: unit.maximumFramesToRender, onRenderFailure: onRenderFailure)
            let listener = listenForRateChange(on: output)
            loop.start()
            self.loop = loop
            return BuiltRoute(
                stopIO: { [weak self] in loop.stop(); self?.loop = nil },
                destroyAggregate: { listener(); aggregate.destroy() },
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

    /// Calls `onSampleRateChange` when the output's nominal rate changes. Returns the remover.
    private func listenForRateChange(on device: AudioObjectID) -> () -> Void {
        var address = HAL.address(kAudioDevicePropertyNominalSampleRate)
        let callback = onSampleRateChange
        let block: AudioObjectPropertyListenerBlock = { _, _ in MainActor.assumeIsolated { callback() } }
        guard AudioObjectAddPropertyListenerBlock(device, &address, .main, block) == noErr else { return {} }
        return {
            var a = address
            AudioObjectRemovePropertyListenerBlock(device, &a, .main, block)
        }
    }
}
