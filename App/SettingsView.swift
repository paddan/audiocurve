import SwiftUI

/// The Settings window (⌘,): background processing, the menu bar ear and launch at login.
struct SettingsView: View {
    @Bindable var model: AppModel

    var body: some View {
        Form {
            Toggle("Process all sound in the background", isOn: $model.backgroundProcessing)
            Text(model.backgroundProcessing
                 ? "AudioCurve keeps processing what your Mac plays when its window is closed."
                 : "AudioCurve processes sound only while its window is open and Bypass is off. Closing the window quits AudioCurve.")
                .font(.callout)
                .foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)
            Toggle("Show in menu bar", isOn: $model.showInMenuBar)
            if !model.showInMenuBar {
                Text("Open AudioCurve from its app icon to get back to the window.")
                    .font(.callout)
                    .foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
            }
            if model.mode.offersLaunchAtLogin {
                Toggle("Launch at login", isOn: $model.launchAtLogin)
            }
        }
        .formStyle(.grouped)
        .frame(width: 420)
        .fixedSize(horizontal: false, vertical: true)
    }
}
