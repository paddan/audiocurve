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
    @State private var status = "Laddar plugin…"
    @State private var playing = false
    private let ticker = Timer.publish(every: 0.05, on: .main, in: .common).autoconnect()

    var body: some View {
        VStack(spacing: 12) {
            HStack {
                Button("Öppna ljudfil…") { openFile() }
                Button(playing ? "Stop" : "Play") {
                    if playing {
                        engine.stop()
                    } else {
                        do { try engine.play(); status = "Spelar (slinga)" }
                        catch { status = "Kunde inte spela: \(error.localizedDescription)" }
                    }
                    playing = engine.isPlaying
                }
                Toggle("Bypass (A/B)", isOn: $bypass)
                    .onChange(of: bypass) { _, v in engine.bypass = v }
                Text(String(format: "%.1f dBFS", peak)).monospacedDigit()
            }
            Text(status).font(.caption).foregroundStyle(.secondary)
            if let pluginVC {
                PluginViewHost(controller: pluginVC).frame(minWidth: 640, minHeight: 380)
            } else {
                Spacer()
            }
        }
        .padding()
        .frame(minWidth: 680, minHeight: 540)
        .onReceive(ticker) { _ in peak = engine.outputPeakDB }
        .task {
            do {
                try await engine.loadPlugin()
                pluginVC = await engine.pluginViewController()
                status = "Plugin laddat"
            } catch {
                status = "Kunde inte ladda plugin: \(error.localizedDescription)"
            }
        }
    }

    private func openFile() {
        let panel = NSOpenPanel()
        panel.allowedContentTypes = [.audio]
        if panel.runModal() == .OK, let url = panel.url {
            do { try engine.open(file: url); playing = false; status = url.lastPathComponent }
            catch { status = "Kunde inte öppna fil: \(error.localizedDescription)" }
        }
    }
}
