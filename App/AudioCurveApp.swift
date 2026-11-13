import SwiftUI

@main
struct AudioCurveApp: App {
    @NSApplicationDelegateAdaptor(AppDelegate.self) private var delegate

    var body: some Scene {
        WindowGroup { HostView(model: delegate.model) }
    }
}

@MainActor
final class AppDelegate: NSObject, NSApplicationDelegate {
    let model = AppModel()

    func applicationDidFinishLaunching(_ notification: Notification) {
        Task { await model.start() }
    }

    func applicationWillTerminate(_ notification: Notification) { model.shutDown() }
}
