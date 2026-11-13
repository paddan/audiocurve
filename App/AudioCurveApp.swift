import SwiftUI

@main
struct AudioCurveApp: App {
    @NSApplicationDelegateAdaptor(AppDelegate.self) private var delegate

    var body: some Scene {
        MenuBarExtra {
            MenuPanel(model: delegate.model)
        } label: {
            Image(nsImage: MenuBarIcon.image(for: delegate.model.state))
        }
        .menuBarExtraStyle(.window)
        MainWindow(model: delegate.model)
    }
}

@MainActor
final class AppDelegate: NSObject, NSApplicationDelegate {
    static let askedLoginKey = "askedLaunchAtLogin"
    let model = AppModel()

    func applicationDidFinishLaunching(_ notification: Notification) {
        Task { await model.start() }
        askLaunchAtLoginOnce()
    }

    func applicationWillTerminate(_ notification: Notification) { model.shutDown() }

    private func askLaunchAtLoginOnce() {
        let d = UserDefaults.standard
        guard !d.bool(forKey: Self.askedLoginKey) else { return }
        d.set(true, forKey: Self.askedLoginKey)
        let alert = NSAlert()
        alert.messageText = String(localized: "Start AudioCurve when you log in?")
        alert.informativeText = String(localized: "AudioCurve lives in the menu bar. You can change this later in its menu.")
        alert.addButton(withTitle: String(localized: "Start at Login"))
        alert.addButton(withTitle: String(localized: "Not Now"))
        NSApp.activate()
        if alert.runModal() == .alertFirstButtonReturn { model.launchAtLogin = true }
    }
}
