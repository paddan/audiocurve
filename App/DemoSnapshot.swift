import AppKit
import SwiftUI

/// Screenshot hook for the README: with `AUDIOCURVE_DEMO=panel` and `AUDIOCURVE_DEMO_OUT=<dir>`
/// the app draws the menu bar panel with made-up data into `<dir>/panel-light.png` and
/// `<dir>/panel-dark.png`, then quits. Drawing in-process needs no screen recording permission.
@MainActor
enum DemoSnapshot {
    static var outputDirectory: String? { ProcessInfo.processInfo.environment["AUDIOCURVE_DEMO_OUT"] }

    static func writeAndQuit(model: AppModel, to dir: String) {
        // Controls draw their accent colors only in a key window of the active app.
        NSApp.setActivationPolicy(.regular)
        NSApp.activate(ignoringOtherApps: true)
        let light = render(model: model, appearance: .aqua)
        let dark = render(model: model, appearance: .darkAqua)
        // Let the windows become key and the switch finish animating before drawing.
        DispatchQueue.main.asyncAfter(deadline: .now() + 1.5) {
            write(light, to: "\(dir)/panel-light.png")
            write(dark, to: "\(dir)/panel-dark.png")
            NSApp.terminate(nil)
        }
    }

    /// A key window far off screen, so controls draw in their active colors.
    private final class KeyWindow: NSWindow {
        override var canBecomeKey: Bool { true }
    }

    private static func render(model: AppModel, appearance: NSAppearance.Name) -> NSView {
        let host = NSHostingView(rootView: MenuPanel(model: model)
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
