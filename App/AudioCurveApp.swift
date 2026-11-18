import AudioCurveCore
import SwiftUI

@main
struct AudioCurveApp: App {
    @NSApplicationDelegateAdaptor(AppDelegate.self) private var delegate

    var body: some Scene {
        @Bindable var model = delegate.model
        MenuBarExtra(isInserted: $model.showInMenuBar) {
            MenuPanel(model: delegate.model, openMain: { [delegate] in delegate.showMainWindow() },
                      settingsWillOpen: { [delegate] in delegate.settingsWillOpen() })
        } label: {
            Image(nsImage: MenuBarIcon.image(for: delegate.model.state))
        }
        .menuBarExtraStyle(.window)
        Settings {
            SettingsView(model: delegate.model)
        }
    }
}

@MainActor
final class AppDelegate: NSObject, NSApplicationDelegate {
    static let askedLoginKey = "askedLaunchAtLogin"
    let model = AppModel()
    private lazy var mainWindow = MainWindowController(model: model)
    /// The Settings window is open: the app needs to be `.regular` for it to come to the front.
    private var settingsOpen = false

    func applicationDidFinishLaunching(_ notification: Notification) {
        mainWindow.onClose = { [weak self] in self?.mainWindowClosed() }
        model.onModeChange = { [weak self] in self?.modeChanged() }
        NotificationCenter.default.addObserver(forName: NSWindow.willCloseNotification, object: nil,
                                               queue: .main) { [weak self] n in
            let id = (n.object as? NSWindow)?.identifier?.rawValue
            MainActor.assumeIsolated {
                guard let self, Self.isSettings(id: id) else { return }
                self.settingsOpen = false
                // After the close: the window is still visible inside `willClose`.
                DispatchQueue.main.async { self.applyActivationPolicy() }
            }
        }
        let quiet = Self.launchedAsLoginItem() || ProcessInfo.processInfo.environment["AUDIOCURVE_DEMO"] != nil
        Task { await model.start() }
        applyActivationPolicy()
        if !quiet { showMainWindow() }
        askLaunchAtLoginOnce()
    }

    /// A Dock click or a double-click in Finder while running: the way back when the ear is hidden.
    func applicationShouldHandleReopen(_ sender: NSApplication, hasVisibleWindows flag: Bool) -> Bool {
        showMainWindow()
        return false
    }

    func applicationWillTerminate(_ notification: Notification) { model.shutDown() }

    func showMainWindow() {
        mainWindow.show()
        applyActivationPolicy()   // after `show`, so the window counts as open
        NSApp.activate()
    }

    /// Before the menu panel opens Settings.
    func settingsWillOpen() {
        settingsOpen = true
        applyActivationPolicy()
        NSApp.activate()
    }

    /// Turning background processing off makes AudioCurve an ordinary app, which has its window.
    private func modeChanged() {
        if model.mode.quitsWhenWindowCloses, !mainWindow.isOpen { showMainWindow() } else { applyActivationPolicy() }
    }

    private func mainWindowClosed() {
        if model.mode.quitsWhenWindowCloses { model.quit() } else { applyActivationPolicy() }
    }

    private func applyActivationPolicy() {
        let policy: NSApplication.ActivationPolicy =
            model.mode.showsDockIcon(windowOpen: mainWindow.isOpen || settingsOpen || Self.settingsVisible())
                ? .regular : .accessory
        if NSApp.activationPolicy() != policy { NSApp.setActivationPolicy(policy) }
    }

    /// Also covers Settings opened with ⌘, from the main window.
    private static func settingsVisible() -> Bool {
        NSApp.windows.contains { $0.isVisible && isSettings(id: $0.identifier?.rawValue) }
    }

    /// SwiftUI names its Settings window `com_apple_SwiftUI_Settings_window`.
    private static func isSettings(id: String?) -> Bool { id?.contains("Settings") ?? false }

    /// True when launched by the login item rather than by the user.
    private static func launchedAsLoginItem() -> Bool {
        guard let event = NSAppleEventManager.shared().currentAppleEvent else { return false }
        return event.eventID == kAEOpenApplication
            && event.paramDescriptor(forKeyword: keyAEPropData)?.enumCodeValue == keyAELaunchedAsLogInItem
    }

    private func askLaunchAtLoginOnce() {
        guard model.mode.offersLaunchAtLogin else { return }
        let d = UserDefaults.standard
        guard !d.bool(forKey: Self.askedLoginKey) else { return }
        d.set(true, forKey: Self.askedLoginKey)
        let alert = NSAlert()
        alert.messageText = String(localized: "Start AudioCurve when you log in?")
        alert.informativeText = String(localized: "AudioCurve processes all sound in the background. You can change this later in Settings.")
        alert.addButton(withTitle: String(localized: "Start at Login"))
        alert.addButton(withTitle: String(localized: "Not Now"))
        NSApp.activate()
        if alert.runModal() == .alertFirstButtonReturn { model.launchAtLogin = true }
    }
}
