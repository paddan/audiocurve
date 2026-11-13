import AudioCurveCore
import SwiftUI

/// The panel that opens from the menu bar ear.
struct MenuPanel: View {
    @Bindable var model: AppModel
    @Environment(\.openWindow) private var openWindow

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack {
                Text(verbatim: "AudioCurve").font(.headline)
                Spacer()
                Toggle("AudioCurve on", isOn: $model.enabled).toggleStyle(.switch).labelsHidden()
            }
            StatusLine(model: model)
            if model.state == .failed(.permission) {
                Button("Open System Settings") { model.openCaptureSettings() }
            }
            if let host = model.otherHosts.first {
                Text("AudioCurve is also running in \(AppModel.appName(for: host)): the sound is processed twice.",
                     comment: "Warning when another host also runs the plugin")
                    .font(.callout)
                    .foregroundStyle(Color(nsColor: .systemOrange))
                    .fixedSize(horizontal: false, vertical: true)
            }
            Divider()
            if !model.profiles.isEmpty {
                Picker("Profile", selection: Binding(get: { model.activeProfileID },
                                                     set: { if let id = $0 { model.loadProfile(id) } })) {
                    if model.activeProfileID == nil { Text("Unsaved profile").tag(UUID?.none) }
                    ForEach(model.profiles) { Text(verbatim: $0.name).tag(Optional($0.id)) }
                }
                Divider()
            }
            Text("Active on").font(.subheadline).foregroundStyle(.secondary)
            ForEach(model.activeOutputs.menuDevices(available: model.availableOutputs)) { d in
                Toggle(isOn: Binding(get: { model.activeOutputs.isChecked(d.uid) },
                                     set: { model.setOutput(d, enabled: $0) })) {
                    HStack(spacing: 4) {
                        Text(verbatim: d.name).lineLimit(1)
                        if d.uid == model.currentOutput?.uid {
                            Text("(current)", comment: "Output device that is in use now").foregroundStyle(.secondary)
                        } else if !model.availableOutputs.contains(where: { $0.uid == d.uid }) {
                            Text("(not connected)", comment: "Remembered output device that is not connected").foregroundStyle(.secondary)
                        }
                    }
                }
                .toggleStyle(.checkbox)
                .disabled(model.activeOutputs.isLastChecked(d.uid, available: model.availableOutputs))
            }
            Divider()
            Button("Open AudioCurve…") {
                openWindow(id: MainWindow.id)
                NSApp.activate()
            }
            Toggle("Launch at login", isOn: $model.launchAtLogin).toggleStyle(.checkbox)
            Button("Quit AudioCurve") { model.quit() }
        }
        .buttonStyle(.borderless)
        .padding(14)
        .frame(width: 300)
    }
}
