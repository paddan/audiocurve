import AudioCurveCore
import SwiftUI

struct PluginViewHost: NSViewControllerRepresentable {
    let controller: NSViewController
    func makeNSViewController(context: Context) -> NSViewController { controller }
    func updateNSViewController(_ vc: NSViewController, context: Context) {}
}

/// The main window: Bypass, the status line and the plugin's own view, or the hearing test.
@MainActor
struct HostView: View {
    @Bindable var model: AppModel
    @ViewState private var pluginVC: NSViewController?

    var body: some View {
        VStack(spacing: 12) {
            HStack {
                Toggle("Bypass (A/B)", isOn: $model.bypass)
                Button("Hearing test…") { model.openHearingTest() }
                    .disabled(model.hearingTest != nil || model.pluginState != .loaded)
                Spacer()
                StatusLine(model: model)
            }
            if let test = model.hearingTest {
                HearingTestView(model: test)
            } else if let pluginVC {
                PluginViewHost(controller: pluginVC).frame(minWidth: 640, minHeight: 640)
            } else {
                Spacer()
            }
        }
        .padding()
        .frame(minWidth: compact ? Self.compactSize.width : 720, minHeight: compact ? Self.compactSize.height : 700)
        .background(WindowSizer(compact: compact, contentSize: Self.compactSize))
        .task(id: model.pluginState) {
            pluginVC = model.pluginState == .loaded ? await model.host.viewController() : nil
        }
    }

    /// The window size while a hearing test is running or paused: the test screen needs far less
    /// room than the plugin view.
    static let compactSize = CGSize(width: 560, height: 580)

    private var compact: Bool {
        switch model.hearingTest?.phase {
        case .running, .paused: true
        default: false
        }
    }
}

/// Shrinks the window to `contentSize` when `compact` turns on, and puts the previous frame back
/// when it turns off. The top-left corner stays where it is.
private struct WindowSizer: NSViewRepresentable {
    let compact: Bool
    let contentSize: CGSize

    final class Coordinator {
        var applied = false
        var savedFrame: NSRect?
    }

    func makeCoordinator() -> Coordinator { Coordinator() }
    func makeNSView(context: Context) -> NSView { NSView() }

    func updateNSView(_ view: NSView, context: Context) {
        let c = context.coordinator, compact = compact, size = contentSize
        guard compact != c.applied else { return }
        c.applied = compact
        // After this update pass, so the new minimum size is already in place.
        DispatchQueue.main.async {
            guard let w = view.window else { return }
            if compact {
                c.savedFrame = w.frame
                var f = w.frameRect(forContentRect: NSRect(origin: .zero, size: size))
                f.origin = NSPoint(x: w.frame.minX, y: w.frame.maxY - f.height)
                w.setFrame(f, display: true, animate: true)
            } else if let saved = c.savedFrame {
                c.savedFrame = nil
                w.setFrame(saved, display: true, animate: true)
            }
        }
    }
}

/// One line describing the route state.
struct StatusLine: View {
    let model: AppModel

    var body: some View {
        Text(text).font(.callout).foregroundStyle(color).lineLimit(1).truncationMode(.middle)
    }

    private var text: String {
        if model.testing { return String(localized: "Hearing test in progress", comment: "Status") }
        return switch model.state {
        case .running: String(localized: "Active – \(model.currentOutput?.name ?? "")", comment: "Status: processing on this output")
        case .paused: String(localized: "Paused – \(model.currentOutput?.name ?? "") is the output", comment: "Status when the current output device is not one the user enabled")
        case .starting: String(localized: "Starting…", comment: "Status: plugin loading or route being built")
        case .off: String(localized: "Off", comment: "Status: AudioCurve turned off")
        case .failed(.permission): String(localized: "Allow AudioCurve to record system audio", comment: "Status: capture permission missing")
        case .failed(.plugin): String(localized: "The AudioCurve plugin could not be loaded", comment: "Status")
        case .failed(.route): String(localized: "The audio route could not be built", comment: "Status")
        }
    }

    private var color: Color {
        if model.testing { return .secondary }
        return switch model.state {
        case .running: .green
        case .paused, .failed: Color(nsColor: .systemOrange)
        case .off, .starting: .secondary
        }
    }
}
