import AudioCurveCore
import SwiftUI

struct PluginViewHost: NSViewControllerRepresentable {
    let controller: NSViewController
    func makeNSViewController(context: Context) -> NSViewController { controller }
    func updateNSViewController(_ vc: NSViewController, context: Context) {}
}

/// The main window: Bypass, the status line and the plugin's own view.
@MainActor
struct HostView: View {
    let model: AppModel
    @ViewState private var pluginVC: NSViewController?
    @ViewState private var bypass = false

    var body: some View {
        VStack(spacing: 12) {
            HStack {
                Toggle("Bypass (A/B)", isOn: $bypass)
                    .onChange(of: bypass) { _, v in model.bypass = v }
                Spacer()
                StatusLine(model: model)
            }
            if let pluginVC {
                PluginViewHost(controller: pluginVC).frame(minWidth: 640, minHeight: 540)
            } else {
                Spacer()
            }
        }
        .padding()
        .frame(minWidth: 720, minHeight: 600)
        .task(id: model.pluginState) {
            pluginVC = model.pluginState == .loaded ? await model.host.viewController() : nil
            bypass = model.bypass
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
        switch model.state {
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
        switch model.state {
        case .running: .green
        case .paused, .failed: Color(nsColor: .systemOrange)
        case .off, .starting: .secondary
        }
    }
}
