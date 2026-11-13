import SwiftUI

/// The main window scene. The app shows a Dock icon only while it is open.
struct MainWindow: Scene {
    static let id = "main"
    let model: AppModel

    var body: some Scene {
        Window("AudioCurve", id: Self.id) {
            HostView(model: model)
                .onAppear { NSApp.setActivationPolicy(.regular) }
                .onDisappear { NSApp.setActivationPolicy(.accessory) }
        }
        .defaultLaunchBehavior(.suppressed)
    }
}
