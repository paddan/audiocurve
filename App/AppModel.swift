import AudioCurveCore
import AppKit
import Observation
import ServiceManagement

/// The app's state: on/off, the route, the plugin's status and launch at login.
@Observable @MainActor
final class AppModel {
    static let enabledKey = "enabled"
    static let pollInterval: TimeInterval = 2
    /// Input silent this long while other apps play: the capture permission is taken as denied.
    static let silenceForDenied: TimeInterval = 3

    var enabled: Bool = UserDefaults.standard.object(forKey: AppModel.enabledKey) as? Bool ?? true {
        didSet {
            UserDefaults.standard.set(enabled, forKey: Self.enabledKey)
            if enabled {
                permission = .unknown   // turning it on again retries a denied permission
                if state == .failed(.plugin) { controller.retryPlugin() }
            }
            updateInputs()
        }
    }
    private(set) var state: RouteTarget = .off
    private(set) var currentOutput: OutputDevice?
    private(set) var availableOutputs: [OutputDevice] = []
    private(set) var activeOutputs = ActiveOutputs()
    private(set) var profiles: [ProfileSummary] = []
    private(set) var activeProfileID: UUID?
    private(set) var otherHosts: [String] = []
    /// Mirrors `host.loadState` so views can observe it.
    private(set) var pluginState: PluginLoadState = .loading
    var launchAtLogin: Bool {
        get { SMAppService.mainApp.status == .enabled }
        set {
            try? newValue ? SMAppService.mainApp.register() : SMAppService.mainApp.unregister()
            loginToggles += 1   // SMAppService is not observable
        }
    }
    private var loginToggles = 0

    let host = PluginHost()
    private let client: PluginClient
    private let monitor = OutputDeviceMonitor()
    private var builder: CoreAudioRouteBuilder!
    private var controller: RouteController!
    private var permission: PermissionState = .unknown
    private var poll: Timer?
    private var silentSince: Date?

    init() {
        client = PluginClient(host: host)
        builder = CoreAudioRouteBuilder(
            unit: { [weak self] in self?.host.audioUnit },
            onRenderFailure: { [weak self] id in Task { @MainActor in self?.controller.pluginDied(routeID: id) } },
            onRouteInvalid: { [weak self] in self?.controller.invalidate() })
        controller = RouteController(builder: builder) { delay, f in
            DispatchQueue.main.asyncAfter(deadline: .now() + delay) { MainActor.assumeIsolated { f() } }
        }
        controller.onStateChange = { [weak self] in self?.state = $0 }
        controller.onReloadPlugin = { [weak self] in
            self?.host.markLoading()   // so no input update writes .loaded before the reload
            Task { await self?.reloadPlugin() }
        }
        host.onLoadStateChange = { [weak self] s in
            self?.pluginState = s
            self?.updateInputs()
        }
        monitor.onChange = { [weak self] in MainActor.assumeIsolated { self?.refreshOutputs() } }
    }

    func start() async {
        // Screenshot hook: `open AudioCurve.app --env AUDIOCURVE_DEMO=panel` shows made-up devices
        // and profiles and processes no audio.
        if ProcessInfo.processInfo.environment["AUDIOCURVE_DEMO"] == "panel" { showDemoData(); return }
        refreshOutputs()
        await host.load()
        afterLoad()
        let t = Timer(timeInterval: Self.pollInterval, repeats: true) { [weak self] _ in
            MainActor.assumeIsolated { self?.tick() }
        }
        RunLoop.main.add(t, forMode: .common)
        poll = t
    }

    func setOutput(_ d: OutputDevice, enabled on: Bool) {
        var a = activeOutputs
        a.setEnabled(d, on, available: availableOutputs)
        activeOutputs = a
        client.send(.setActiveOutputs(a))
        updateInputs()
    }

    func loadProfile(_ id: UUID) {
        client.send(.loadProfile(id))
        activeProfileID = id
    }

    /// Unmutes at once, saves the plugin state and quits.
    func quit() {
        shutDown()
        NSApp.terminate(nil)
    }

    /// For `applicationWillTerminate`.
    func shutDown() {
        poll?.invalidate()
        controller.shutDown()
        host.saveState()
    }

    func openCaptureSettings() {
        if let url = URL(string: "x-apple.systempreferences:com.apple.settings.PrivacySecurity.extension?Privacy_AudioCapture") {
            NSWorkspace.shared.open(url)
        }
    }

    /// The display name of the app with this bundle ID, or "another app".
    static func appName(for bundleID: String) -> String {
        if let url = NSWorkspace.shared.urlForApplication(withBundleIdentifier: bundleID) {
            return FileManager.default.displayName(atPath: url.path).replacingOccurrences(of: ".app", with: "")
        }
        return String(localized: "another app", comment: "Used when the other host is not known")
    }

    var bypass: Bool {
        get { host.bypass }
        set { host.bypass = newValue }
    }

    private func showDemoData() {
        let airPods = OutputDevice(uid: "demo.airpods", name: "AirPods Pro")
        let hearingAids = OutputDevice(uid: "demo.hearing-aids", name: "Hearing Aids")
        let speakers = OutputDevice(uid: "demo.speakers", name: "Mac Studio Speakers")
        availableOutputs = [airPods, hearingAids, speakers]
        currentOutput = airPods
        activeOutputs = ActiveOutputs(devices: [airPods, speakers])
        let example = ProfileSummary(id: UUID(), name: "Example")
        profiles = [example, ProfileSummary(id: UUID(), name: "Example (quiet room)")]
        activeProfileID = example.id
        state = .running(deviceUID: airPods.uid)
        if let dir = DemoSnapshot.outputDirectory { DemoSnapshot.writeAndQuit(model: self, to: dir) }
    }

    private func reloadPlugin() async {
        await host.reload()
        afterLoad()
    }

    private func afterLoad() {
        client.send(.identifyHost(Bundle.main.bundleIdentifier ?? "se.lindefors.audiocurve"))
        refreshStatus()
        updateInputs()
    }

    private func refreshOutputs() {
        currentOutput = monitor.current
        availableOutputs = monitor.available
        updateInputs()
    }

    private func refreshStatus() {
        guard let s = client.status() else {
            // A loaded plugin that no longer answers has died (also while paused, with no
            // render loop to notice it).
            if host.loadState == .loaded, state != .failed(.plugin) { controller.pluginDied() }
            return
        }
        profiles = s.profiles
        activeProfileID = s.activeProfileID
        otherHosts = s.otherHosts
        if s.activeOutputs != activeOutputs {
            activeOutputs = s.activeOutputs
            updateInputs()
        }
    }

    private func tick() {
        refreshStatus()
        checkPermission()
    }

    /// No public API tells whether recording was denied: a denied tap delivers silence. Taken as
    /// denied when the input stays silent for `silenceForDenied` while other apps play.
    private func checkPermission() {
        guard case .running = state, let loop = builder.loop else { silentSince = nil; return }
        if let s = loop.secondsSinceSignal, s < Self.silenceForDenied {
            silentSince = nil
            if permission != .granted { permission = .granted }
            return
        }
        guard permission != .granted, HAL.othersPlaying() else { silentSince = nil; return }
        let since = silentSince ?? Date()
        silentSince = since
        if Date().timeIntervalSince(since) >= Self.silenceForDenied {
            permission = .denied
            silentSince = nil
            updateInputs()
        }
    }

    private func updateInputs() {
        controller.inputs = RouteInputs(enabled: enabled, permission: permission, plugin: host.loadState,
                                        outputUID: currentOutput?.uid, activeOutputs: activeOutputs)
    }
}
