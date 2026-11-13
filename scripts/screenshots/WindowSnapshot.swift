import AppKit
import AudioCurveCore
import AVFoundation
import SwiftUI

/// README screenshots of the plugin window: `make screenshots`.
///
/// Built from the AU's own sources into a plain executable, so the plugin view is drawn in
/// this process (no screen recording permission). It runs the real kernel on generated noise
/// with a made-up example audiogram, and writes `<pane>-light.png` and `<pane>-dark.png` for
/// the audiogram, gain and spectrum panes. `CFFIXED_USER_HOME` must point at an empty folder,
/// so the plugin's profile store never touches the user's own profiles.
@main
@MainActor
enum WindowSnapshot {
    static let size = CGSize(width: 900, height: 720)

    static func main() {
        let env = ProcessInfo.processInfo.environment
        let realHome = String(cString: getpwuid(getuid())!.pointee.pw_dir)
        guard let home = env["CFFIXED_USER_HOME"], home != realHome, NSHomeDirectory() == home,
              let out = CommandLine.arguments.dropFirst().first else {
            FileHandle.standardError.write(Data("usage: CFFIXED_USER_HOME=<empty dir> screenshots <out dir>\n".utf8))
            exit(2)
        }
        let app = NSApplication.shared
        app.setActivationPolicy(.regular)
        DispatchQueue.main.async { run(out: out) }
        app.run()
    }

    static func run(out: String) {
        do {
            let unit = try AudioCurveAudioUnit(componentDescription: AudioComponentDescription(
                componentType: kAudioUnitType_Effect, componentSubType: 0x68636D70,
                componentManufacturer: 0x4C6E6473, componentFlags: 0, componentFlagsMask: 0))
            unit.profile = example
            unit.setParameter(.tone, 25)
            try unit.allocateRenderResources()
            // Let the main-queue recompute push the fitting to the kernel before rendering.
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.3) {
                render(unit, seconds: 6)
                NSApp.activate(ignoringOtherApps: true)
                var shots: [(path: String, pane: Pane, appearance: NSAppearance.Name)] = []
                for pane in [Pane.audiogram, .gain, .spectrum] {
                    for (name, appearance) in [("light", NSAppearance.Name.aqua), ("dark", .darkAqua)] {
                        shots.append(("\(out)/\(pane.rawValue)-\(name).png", pane, appearance))
                    }
                }
                capture(unit, shots[...], count: shots.count, out: out)
            }
        } catch {
            FileHandle.standardError.write(Data("screenshots: \(error)\n".utf8))
            exit(1)
        }
    }

    /// One window at a time: only the key window draws selected segments in the accent color.
    static func capture(_ unit: AudioCurveAudioUnit, _ shots: ArraySlice<(path: String, pane: Pane, appearance: NSAppearance.Name)>,
                        count: Int, out: String) {
        guard let shot = shots.first else {
            print("Wrote \(count) screenshots to \(out)")
            exit(0)
        }
        let view = draw(unit, pane: shot.pane, appearance: shot.appearance)
        DispatchQueue.main.asyncAfter(deadline: .now() + 1) {
            write(view, to: shot.path)
            view.window?.orderOut(nil)
            capture(unit, shots.dropFirst(), count: count, out: out)
        }
    }

    /// A made-up, mildly sloping example audiogram (not anyone's real hearing).
    static var example: HearingProfile {
        let left: [Double] = [10, 10, 15, 20, 25, 30, 35, 45, 55, 60, 65]
        let right: [Double] = [5, 10, 10, 15, 20, 25, 30, 40, 50, 55, 60]
        func audiogram(_ t: [Double]) -> Audiogram {
            Audiogram(points: zip(Audiogram.standardFrequencies, t).map { AudiogramPoint(frequencyHz: $0, thresholdDBHL: $1) })
        }
        var p = HearingProfile.flat()
        p.setAudiogram(audiogram(left), for: .left)
        p.setAudiogram(audiogram(right), for: .right)
        return p
    }

    /// Runs pink noise with music-like level changes through the kernel, so meters and spectrum
    /// have something to show.
    static func render(_ unit: AudioCurveAudioUnit, seconds: Double) {
        let frames: AVAudioFrameCount = 512
        let format = AVAudioFormat(standardFormatWithSampleRate: 48000, channels: 2)!
        let buffer = AVAudioPCMBuffer(pcmFormat: format, frameCapacity: frames)!
        buffer.frameLength = frames
        var rng = SystemRandomNumberGenerator()
        var b = [Float](repeating: 0, count: 7)
        var n = 0
        let block = unit.internalRenderBlock
        let pull: AURenderPullInputBlock = { _, _, count, _, abl in
            let list = UnsafeMutableAudioBufferListPointer(abl)
            for i in 0..<Int(count) {
                // Paul Kellet's pink noise filter.
                let w = Float.random(in: -1...1, using: &rng)
                b[0] = 0.99886 * b[0] + w * 0.0555179; b[1] = 0.99332 * b[1] + w * 0.0750759
                b[2] = 0.96900 * b[2] + w * 0.1538520; b[3] = 0.86650 * b[3] + w * 0.3104856
                b[4] = 0.55000 * b[4] + w * 0.5329522; b[5] = -0.7616 * b[5] - w * 0.0168980
                let pink = (b[0] + b[1] + b[2] + b[3] + b[4] + b[5] + b[6] + w * 0.5362) * 0.11
                b[6] = w * 0.115926
                let t = Float(n) / 48000
                let gain = 0.18 * pow(10, (6 * sin(2 * .pi * 3 * t)) / 20)
                n += 1
                for ch in list { ch.mData!.assumingMemoryBound(to: Float.self)[i] = pink * gain }
            }
            return noErr
        }
        var flags = AudioUnitRenderActionFlags()
        var time = AudioTimeStamp()
        for _ in 0..<Int(seconds * 48000 / Double(frames)) {
            _ = block(&flags, &time, frames, 0, buffer.mutableAudioBufferList, nil, pull)
            time.mSampleTime += Double(frames)
        }
    }

    private final class KeyWindow: NSWindow {
        override var canBecomeKey: Bool { true }
    }

    static func draw(_ unit: AudioCurveAudioUnit, pane: Pane, appearance: NSAppearance.Name) -> NSView {
        let vm = PluginViewModel(audioUnit: unit)
        unit.onOutputsChange = nil
        let airPods = OutputDevice(uid: "demo.airpods", name: "AirPods Pro")
        let speakers = OutputDevice(uid: "demo.speakers", name: "Mac Studio Speakers")
        vm.currentOutput = airPods
        vm.availableOutputs = [airPods, OutputDevice(uid: "demo.hearing-aids", name: "Hearing Aids"), speakers]
        vm.activeOutputs = ActiveOutputs(devices: [airPods, speakers])
        vm.processingActive = true
        vm.view = pane
        vm.meters = unit.readMeters()
        vm.spectrum = unit.readSpectrum()
        let root = VStack(spacing: 12) {
            HStack {
                Toggle("Bypass (A/B)", isOn: .constant(false))
                Spacer()
                Text(verbatim: "Active – AirPods Pro").font(.callout).foregroundStyle(.green)
            }
            PluginView(vm: vm)
        }
        .padding()
        .frame(width: size.width, height: size.height)
        .environment(\.controlActiveState, .key)
        .tint(.accentColor)
        .background(Color(nsColor: .windowBackgroundColor))
        let host = NSHostingView(rootView: root)
        let window = KeyWindow(contentRect: NSRect(origin: NSPoint(x: -10_000, y: -10_000), size: size),
                               styleMask: .borderless, backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false
        window.appearance = NSAppearance(named: appearance)
        window.contentView = host
        host.frame = NSRect(origin: .zero, size: size)
        host.layoutSubtreeIfNeeded()
        window.makeKeyAndOrderFront(nil)
        return host
    }

    static func write(_ host: NSView, to path: String) {
        guard let rep = host.bitmapImageRepForCachingDisplay(in: host.bounds) else { return }
        host.cacheDisplay(in: host.bounds, to: rep)
        try? rep.representation(using: .png, properties: [:])?.write(to: URL(fileURLWithPath: path))
    }
}
