import SwiftUI
import Combine
import UniformTypeIdentifiers

struct PluginViewHost: NSViewControllerRepresentable {
    let controller: NSViewController
    func makeNSViewController(context: Context) -> NSViewController { controller }
    func updateNSViewController(_ vc: NSViewController, context: Context) {}
}

@MainActor
struct HostView: View {
    @State private var engine = PlayerEngine()
    @State private var pluginVC: NSViewController?
    @State private var bypass = false
    @State private var peak: Float = -120
    @State private var status = String(localized: "Loading plugin…")
    @State private var playing = false
    private let ticker = Timer.publish(every: 0.05, on: .main, in: .common).autoconnect()

    var body: some View {
        VStack(spacing: 12) {
            HStack {
                Button("Open Audio File…") { openFile() }
                Button(playing ? LocalizedStringKey("Stop") : LocalizedStringKey("Play")) {
                    if playing {
                        engine.stop()
                    } else {
                        do { try engine.play(); status = String(localized: "Playing (loop)") }
                        catch { status = String(localized: "Could not play: \(error.localizedDescription)") }
                    }
                    playing = engine.isPlaying
                }
                Toggle("Bypass (A/B)", isOn: $bypass)
                    .onChange(of: bypass) { _, v in engine.bypass = v }
                Text(verbatim: String(format: "%.1f dBFS", peak)).monospacedDigit()
            }
            Text(status).font(.caption).foregroundStyle(.secondary)
            if let pluginVC {
                PluginViewHost(controller: pluginVC).frame(minWidth: 640, minHeight: 540)
            } else {
                Spacer()
            }
        }
        .padding()
        .frame(minWidth: 680, minHeight: 700)
        .onReceive(ticker) { _ in peak = engine.outputPeakDB }
        .task {
            do {
                try await engine.loadPlugin()
                pluginVC = await engine.pluginViewController()
                status = String(localized: "Plugin loaded")
                #if DEBUG
                // Screenshot/demo hook: AUDIOCURVE_DEMO_FILE=<audio file> opens and loops it.
                if let path = ProcessInfo.processInfo.environment["AUDIOCURVE_DEMO_FILE"] {
                    try engine.open(file: URL(fileURLWithPath: path))
                    try engine.play()
                    playing = engine.isPlaying
                    status = String(localized: "Playing (loop)")
                }
                #endif
            } catch {
                status = String(localized: "Could not load plugin: \(error.localizedDescription)")
            }
        }
    }

    private func openFile() {
        let panel = NSOpenPanel()
        panel.allowedContentTypes = [.audio]
        if panel.runModal() == .OK, let url = panel.url {
            do { try engine.open(file: url); playing = false; status = url.lastPathComponent }
            catch { status = String(localized: "Could not open file: \(error.localizedDescription)") }
        }
    }
}
