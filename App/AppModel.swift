import AudioCurveCore
import AppKit
import Observation
import ServiceManagement

/// The app's state: on/off, the route, the plugin's status and launch at login.
@Observable @MainActor
final class AppModel {
    static let enabledKey = "enabled"
    static let backgroundKey = "backgroundProcessing"
    static let menuBarKey = "showInMenuBar"
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
    private(set) var assignments = OutputAssignments()
    private(set) var profiles: [ProfileSummary] = []
    private(set) var activeProfileID: UUID?
    private(set) var otherHosts: [String] = []
    /// True while a hearing test owns the output: the route is paused so the tones are never
    /// captured or processed.
    var testing = false {
        didSet { if testing != oldValue { updateInputs() } }
    }
    /// The hearing test shown in the window instead of the plugin view, or nil.
    private(set) var hearingTest: HearingTestModel?
    /// Mirrors `host.loadState` so views can observe it.
    private(set) var pluginState: PluginLoadState = .loading
    /// SMAppService is not observable, so the status is mirrored here: after every change and
    /// on every poll (it can also change in System Settings).
    private(set) var launchAtLoginEnabled = SMAppService.mainApp.status == .enabled
    var launchAtLogin: Bool {
        get { launchAtLoginEnabled }
        set {
            do {
                try newValue ? SMAppService.mainApp.register() : SMAppService.mainApp.unregister()
            } catch {
                NSLog("AudioCurve: launch at login %@ failed: %@", newValue ? "register" : "unregister",
                      error.localizedDescription)
            }
            refreshLaunchAtLogin()
        }
    }

    private func refreshLaunchAtLogin() {
        let on = SMAppService.mainApp.status == .enabled
        if on != launchAtLoginEnabled { launchAtLoginEnabled = on }
    }

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
        bypassOn = mode.bypassAtStart
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
        // `AUDIOCURVE_DEMO=hearingtest` draws the hearing test screens instead.
        if let demo = ProcessInfo.processInfo.environment["AUDIOCURVE_DEMO"], ["panel", "hearingtest"].contains(demo) {
            showDemoData(demo)
            return
        }
        refreshOutputs()   // also restores a volume a crashed test left raised
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

    /// Assigns a saved profile to an output (nil removes it). The local copy follows at once so
    /// the picker does not flicker until the next poll.
    func assign(_ d: OutputDevice, profile id: UUID?) {
        if let id {
            assignments.assign(d, profile: id, levels: assignments.assignment(d.uid)?.levels)
        } else {
            assignments.byUID[d.uid] = nil
        }
        client.send(.assignProfile(d, id))
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
        // After the route: ending the test clears `testing`, which must not rebuild it.
        hearingTest?.shutDown()
        host.saveState()
    }

    /// Shows the hearing test in the window.
    func openHearingTest() {
        if hearingTest == nil { hearingTest = HearingTestModel(app: self) }
    }

    /// Back to the plugin view. Stops a running test first.
    func closeHearingTest() {
        hearingTest?.shutDown()
        hearingTest = nil
    }

    /// Saves a measured profile in the plugin (and assigns it to `assignTo`).
    func saveMeasured(id: UUID, name: String, profile: HearingProfile, measurement: HearingTestRecord,
                      assignTo: OutputDevice?) {
        client.send(.saveMeasuredProfile(id: id, name: name, profile: profile, measurement: measurement,
                                         assignTo: assignTo))
    }

    /// Reads the plugin's status now instead of waiting for the next poll.
    func refreshStatusNow() { refreshStatus() }

    /// Volumes the hearing test raised and could not restore (the app crashed, or the device
    /// disappeared mid-test): each is restored when its device is present and forgotten once the
    /// restore is done (`VolumeSnapshot.remaining`: a write that keeps failing is given up after a
    /// few attempts). `except` is the device a running test holds at max.
    static func restorePendingVolumes(except heldUID: String? = nil) {
        for s in VolumeSnapshot.pending() where s.deviceUID != heldUID && HAL.device(uid: s.deviceUID) != nil {
            VolumeSnapshot.updatePending(uid: s.deviceUID, remaining: DeviceVolume.restore(s))
        }
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

    /// The plugin's bypass, mirrored so views follow when the mode sets it.
    private(set) var bypassOn = false
    var bypass: Bool {
        get { bypassOn }
        set {
            host.bypass = newValue
            if bypassOn != newValue { bypassOn = newValue }
        }
    }

    /// On: a background menu bar app that processes all system audio. Off: an ordinary app that
    /// processes sound while Bypass is off and quits when its window closes.
    var backgroundProcessing: Bool = UserDefaults.standard.object(forKey: AppModel.backgroundKey) as? Bool ?? true {
        didSet {
            guard backgroundProcessing != oldValue else { return }
            UserDefaults.standard.set(backgroundProcessing, forKey: Self.backgroundKey)
            bypass = mode.bypassAtStart
            if !mode.offersLaunchAtLogin, launchAtLogin { launchAtLogin = false }
            onModeChange?()
        }
    }
    var mode: AppMode { AppMode(background: backgroundProcessing) }
    /// Called after `backgroundProcessing` changes (the delegate updates the Dock icon).
    var onModeChange: (() -> Void)?

    var showInMenuBar: Bool = UserDefaults.standard.object(forKey: AppModel.menuBarKey) as? Bool ?? true {
        didSet { UserDefaults.standard.set(showInMenuBar, forKey: Self.menuBarKey) }
    }

    private func showDemoData(_ demo: String) {
        let airPods = OutputDevice(uid: "demo.airpods", name: "AirPods Pro")
        let hearingAids = OutputDevice(uid: "demo.hearing-aids", name: "Hearing Aids")
        let speakers = OutputDevice(uid: "demo.speakers", name: "Mac Studio Speakers")
        availableOutputs = [airPods, hearingAids, speakers]
        currentOutput = airPods
        activeOutputs = ActiveOutputs(devices: [airPods, speakers])
        let example = ProfileSummary(id: UUID(), name: "Example")
        profiles = [example, ProfileSummary(id: UUID(), name: "Example (quiet room)")]
        activeProfileID = example.id
        assignments.assign(airPods, profile: example.id, levels: nil)
        assignments.assign(speakers, profile: profiles[1].id, levels: nil)
        state = .running(deviceUID: airPods.uid)
        guard let dir = DemoSnapshot.outputDirectory else { return }
        if demo == "hearingtest" {
            DemoSnapshot.writeHearingTestAndQuit(app: self, device: airPods, to: dir)
        } else {
            DemoSnapshot.writeAndQuit(model: self, to: dir)
        }
    }

    private func reloadPlugin() async {
        await host.reload()
        afterLoad()
    }

    private func afterLoad() {
        client.send(.identifyHost(Bundle.main.bundleIdentifier ?? AppIdentity.bundleID))
        host.bypass = bypassOn   // also after a reload, which starts the AU unbypassed
        refreshStatus()
        updateInputs()
    }

    private func refreshOutputs() {
        currentOutput = monitor.current
        availableOutputs = monitor.available
        updateInputs()
        hearingTest?.checkEnvironment()
        // A device that left mid-test may be back: give it its volume.
        Self.restorePendingVolumes(except: hearingTest?.heldVolumeUID)
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
        if s.otherHosts != otherHosts {
            otherHosts = s.otherHosts
            hearingTest?.checkEnvironment()
        }
        if s.assignments != assignments { assignments = s.assignments }
        if s.activeOutputs != activeOutputs {
            activeOutputs = s.activeOutputs
            updateInputs()
        }
    }

    private func tick() {
        refreshStatus()
        // Retries a restore that failed while a reconnected device was not ready yet.
        Self.restorePendingVolumes(except: hearingTest?.heldVolumeUID)
        checkPermission()
        refreshLaunchAtLogin()
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
                                        outputUID: currentOutput?.uid, activeOutputs: activeOutputs,
                                        testing: testing)
    }
}
