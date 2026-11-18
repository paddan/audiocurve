import AppKit
import AudioCurveCore
import SwiftUI

/// Screenshot hooks for the README. Drawing in-process needs no screen recording permission, and
/// the app processes no audio in these modes. With `AUDIOCURVE_DEMO_OUT=<dir>`:
/// - `AUDIOCURVE_DEMO=panel` draws the menu bar panel into `<dir>/panel-light.png` and `-dark.png`.
/// - `AUDIOCURVE_DEMO=hearingtest` draws a running hearing test and a result into
///   `<dir>/hearingtest-running-*.png` and `<dir>/hearingtest-result-*.png`.
/// Both use made-up data and quit when done.
@MainActor
enum DemoSnapshot {
    static var outputDirectory: String? { ProcessInfo.processInfo.environment["AUDIOCURVE_DEMO_OUT"] }

    static func writeAndQuit(model: AppModel, to dir: String) {
        writeAndQuit(["panel": AnyView(MenuPanel(model: model))], to: dir)
    }

    static func writeHearingTestAndQuit(app: AppModel, device: OutputDevice, to dir: String) {
        let date = DateComponents(calendar: .current, year: 2026, month: 11, day: 16).date ?? Date()
        let running = HearingTestModel(app: app)
        running.showDemo(device: device, date: date,
                         phase: .running(Presentation(ear: .right, frequencyHz: 2000, levelHL: 30,
                                                      levelDBFS: -50, gapBeforeS: 2, index: 9)),
                         result: nil)
        let result = exampleResult
        let done = HearingTestModel(app: app)
        done.showDemo(device: device, date: date, phase: .done(result), result: result)
        writeAndQuit([
            // The window's size while the test runs (`HostView.compactSize`, less its padding and
            // the Bypass row), and its usual size for the result.
            "hearingtest-running": AnyView(HearingTestView(model: running).padding().frame(width: 560, height: 520)),
            "hearingtest-result": AnyView(HearingTestView(model: done).padding().frame(width: 720, height: 580)),
        ], to: dir)
    }

    /// The README's mildly sloping example audiogram, right ear 5 dB better in the treble.
    private static var exampleResult: HearingTestResult {
        let left: [Double] = [10, 10, 15, 15, 20, 25, 25, 35, 40, 45, 45]
        let right: [Double] = [10, 10, 10, 15, 15, 20, 20, 30, 35, 40, 40]
        func points(_ hl: [Double]) -> [TestPoint] {
            zip(Audiogram.standardFrequencies, hl).map { TestPoint(frequencyHz: $0, dBFS: $1 - 100, hl: $1) }
        }
        return HearingTestResult(left: points(left), right: points(right), flags: [])
    }

    private static func writeAndQuit(_ views: [String: AnyView], to dir: String) {
        // Controls draw their accent colors only in a key window of the active app.
        NSApp.setActivationPolicy(.regular)
        NSApp.activate(ignoringOtherApps: true)
        let hosts = views.flatMap { name, view in
            [("\(name)-light", render(view, appearance: .aqua)), ("\(name)-dark", render(view, appearance: .darkAqua))]
        }
        // Let the windows become key and animations finish before drawing.
        DispatchQueue.main.asyncAfter(deadline: .now() + 1.5) {
            for (name, host) in hosts { write(host, to: "\(dir)/\(name).png") }
            NSApp.terminate(nil)
        }
    }

    /// A key window far off screen, so controls draw in their active colors.
    private final class KeyWindow: NSWindow {
        override var canBecomeKey: Bool { true }
    }

    private static func render(_ view: AnyView, appearance: NSAppearance.Name) -> NSView {
        let host = NSHostingView(rootView: view
            .environment(\.controlActiveState, .key)
            .tint(.accentColor)
            .background(Color(nsColor: .windowBackgroundColor)))
        let window = KeyWindow(contentRect: NSRect(origin: NSPoint(x: -10_000, y: -10_000), size: host.fittingSize),
                               styleMask: .borderless, backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false
        window.appearance = NSAppearance(named: appearance)
        window.contentView = host
        host.frame = NSRect(origin: .zero, size: host.fittingSize)
        host.layoutSubtreeIfNeeded()
        window.makeKeyAndOrderFront(nil)
        return host
    }

    private static func write(_ host: NSView, to path: String) {
        guard let rep = host.bitmapImageRepForCachingDisplay(in: host.bounds) else { return }
        host.cacheDisplay(in: host.bounds, to: rep)
        try? rep.representation(using: .png, properties: [:])?.write(to: URL(fileURLWithPath: path))
    }
}
