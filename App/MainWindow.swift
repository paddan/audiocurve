import AppKit
import SwiftUI

/// The main window, owned by AppKit so the app can show it at launch, on reopen and from the
/// menu bar panel, and quit when it closes outside background mode.
@MainActor
final class MainWindowController: NSObject, NSWindowDelegate {
    private let model: AppModel
    private var window: NSWindow?
    /// Called after the window closed.
    var onClose: (() -> Void)?

    init(model: AppModel) { self.model = model }

    /// Visible or in the Dock (a minimised window keeps the Dock icon).
    var isOpen: Bool { window.map { $0.isVisible || $0.isMiniaturized } ?? false }

    func show() {
        let w = window ?? makeWindow()
        window = w
        w.makeKeyAndOrderFront(nil)
        NSApp.activate()
    }

    func windowWillClose(_ notification: Notification) {
        model.hearingTest?.windowClosed()
        // After the close: `isVisible` is still true inside `windowWillClose`.
        DispatchQueue.main.async { [weak self] in self?.onClose?() }
    }

    private func makeWindow() -> NSWindow {
        let w = NSWindow(contentViewController: NSHostingController(rootView: HostView(model: model)))
        w.title = "AudioCurve"
        w.styleMask = [.titled, .closable, .miniaturizable, .resizable]
        w.isReleasedWhenClosed = false
        w.delegate = self
        w.setFrameAutosaveName("main")
        if !w.setFrameUsingName("main") { w.center() }
        return w
    }
}
