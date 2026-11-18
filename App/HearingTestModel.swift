import AudioCurveCore
import Foundation
import Observation

/// Runs the hearing test: drives `HearingTest`, plays its tones with `ToneGenerator` and owns the
/// device volume while the test runs.
///
/// Safety order before the first tone: the route is paused (`AppModel.testing`), the generator
/// starts silent, the original volume is saved (`VolumeSnapshot.savePending`), then raised to max.
/// The volume is raised only while the test is running: whenever it pauses or ends — pause,
/// finish, cancel, stop, quit — the tones go off first, then the volume is restored and the pending
/// snapshot cleared. A restore that fails (device gone) keeps the snapshot, so it is restored when
/// the device is back (also after a relaunch); raising again adopts it rather than re-reading.
@Observable @MainActor
final class HearingTestModel {
    enum Phase: Equatable {
        case intro
        case running(Presentation)
        case paused(PauseReason)
        case done(HearingTestResult)
        case saving(UUID)
        case saveFailed
    }

    enum PauseReason: Equatable { case user, otherSound }

    /// Why a running test stopped; shown on the intro screen.
    enum StopReason: Equatable {
        case outputChanged, otherHost, windowClosed, generatorStopped, generatorFailed, volumeNotSet, lowSampleRate
    }

    /// Time to answer after the tone ends.
    static let responseWindow: TimeInterval = 1.5
    static let checkInterval: TimeInterval = 1
    static let savePollInterval: TimeInterval = 0.25
    static let saveTimeout: TimeInterval = 3
    /// Granularity of the waits inside a presentation.
    private static let tick: TimeInterval = 0.02

    private(set) var phase: Phase = .intro
    private(set) var stopReason: StopReason?
    /// The result, kept through saving and a failed save.
    private(set) var result: HearingTestResult?
    /// The output the test runs on, fixed at start.
    private(set) var device: OutputDevice?
    private(set) var testDate = Date()
    /// The user's transducer choice; nil follows `TestCalibration.defaultTransducer`.
    var transducerChoice: Transducer?

    // Checks for the current output, refreshed every second on the intro and while paused.
    private(set) var othersPlaying = false
    private(set) var muted = false
    private(set) var volumeSettable = true
    /// The output runs below `ToneGenerator.minimumSampleRate`: the test cannot run.
    private(set) var lowSampleRate = false
    /// A test ended on an output whose volume AudioCurve cannot set: the user raised it by hand.
    private(set) var remindVolumeDown = false

    @ObservationIgnored private unowned let app: AppModel
    @ObservationIgnored private var test: HearingTest?
    @ObservationIgnored private var calibration: TestCalibration?
    @ObservationIgnored private var generator: ToneGenerator?
    /// The volume to give back; nil when nothing is raised.
    @ObservationIgnored private var snapshot: VolumeSnapshot?
    /// This test runs on an output without a settable volume (the user set it to max).
    @ObservationIgnored private var manualVolume = false
    @ObservationIgnored private var runTask: Task<Void, Never>?
    @ObservationIgnored private var checkTask: Task<Void, Never>?
    @ObservationIgnored private var saveTask: Task<Void, Never>?
    /// True from the tone's start until its response window closes.
    @ObservationIgnored private var windowOpen = false
    @ObservationIgnored private var heard = false
    /// Reused by a retry so the plugin never saves the result twice.
    @ObservationIgnored private var saveID: UUID?

    init(app: AppModel) {
        self.app = app
        refreshIntroChecks()
        checkTask = Task { [weak self] in
            while !Task.isCancelled {
                try? await Task.sleep(for: .seconds(Self.checkInterval))
                guard let self, !Task.isCancelled else { return }
                self.checkEnvironment()
            }
        }
    }

    // MARK: Intro

    var output: OutputDevice? { app.currentOutput }

    var transducer: Transducer {
        transducerChoice ?? TestCalibration.defaultTransducer(deviceName: output?.name ?? "")
    }

    /// The app running another AudioCurve instance, if any: the test cannot start then.
    var blockingHostName: String? { app.otherHosts.first.map(AppModel.appName(for:)) }

    var canStart: Bool {
        phase == .intro && output != nil && app.otherHosts.isEmpty && !muted && !othersPlaying && !lowSampleRate
    }

    /// Paused, and nothing would make the raised volume unsafe or the tones inaudible: the same
    /// checks as Start.
    var canResume: Bool {
        guard case .paused = phase else { return false }
        return !muted && !othersPlaying && !lowSampleRate
    }

    /// The device whose volume this test holds raised (its pending snapshot must not be restored
    /// behind the test's back), or nil.
    var heldVolumeUID: String? { snapshot?.deviceUID }

    var isActive: Bool {
        switch phase {
        case .running, .paused: true
        default: false
        }
    }

    // MARK: Commands

    func start() {
        guard phase == .intro, let out = output, app.otherHosts.isEmpty else { return }
        refreshIntroChecks()
        guard !muted, !othersPlaying, !lowSampleRate else { return }
        stopReason = nil
        remindVolumeDown = false
        manualVolume = false
        AppModel.restorePendingVolumes()   // an older test's volume first, so the saved one is the user's
        app.testing = true
        device = out
        do {
            let g = try ToneGenerator(deviceUID: out.uid)
            generator = g
            try g.start()
        } catch {
            NSLog("AudioCurve: hearing test generator failed: %@", String(describing: error))
            endActive()
            stopReason = .generatorFailed
            return
        }
        guard raiseVolume() else {
            endActive()
            stopReason = .volumeNotSet
            return
        }
        let cal = TestCalibration.nominal(transducer)
        calibration = cal
        test = HearingTest(calibration: cal, random: { Double.random(in: 0..<1) })
        testDate = Date()
        run()
    }

    /// "I hear it". Counts only while a tone or its response window is open.
    func respond() {
        guard case .running = phase, windowOpen else { return }
        heard = true
    }

    func pause() { pause(.user) }

    /// Starts the generator and raises the volume again, unless another app plays, the output is
    /// muted or runs at a low sample rate (checked now, as Start does).
    func resume() {
        guard case .paused = phase, let g = generator else { return }
        refreshIntroChecks()
        guard canResume else { return }
        do { try g.start() } catch {
            NSLog("AudioCurve: hearing test generator failed on resume: %@", String(describing: error))
            stop(.generatorStopped)
            return
        }
        guard raiseVolume() else { stop(.volumeNotSet); return }
        run()
    }

    /// Ends the test (tones off, volume back) and leaves the test view. On an output whose volume
    /// the user raised by hand, a running test goes back to the intro instead, which asks them to
    /// turn it down.
    func cancel() {
        if isActive, manualVolume {
            stop(nil)
            return
        }
        app.closeHearingTest()
    }

    /// Leaves the result unsaved.
    func discard() { app.closeHearingTest() }

    /// The window closed: a running test stops.
    func windowClosed() { stop(.windowClosed) }

    func save(name: String, assign: Bool) {
        guard let result, let device, let calibration else { return }
        switch phase {
        case .done, .saveFailed: break
        default: return
        }
        let id = saveID ?? UUID()
        saveID = id
        let zeros = Array(repeating: 0.0, count: Bands.count)
        let profile = HearingProfile(left: result.audiogram(.left), right: result.audiogram(.right),
                                     leftTrimDB: zeros, rightTrimDB: zeros)
        let record = HearingTestRecord(
            date: testDate, output: device, calibration: calibration, left: result.left, right: result.right,
            flags: result.flags,
            appVersion: Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? "")
        let trimmed = name.trimmingCharacters(in: .whitespacesAndNewlines)
        app.saveMeasured(id: id, name: trimmed.isEmpty ? defaultName : trimmed, profile: profile,
                         measurement: record, assignTo: assign ? device : nil)
        phase = .saving(id)
        saveTask?.cancel()
        saveTask = Task { [weak self] in
            let deadline = ContinuousClock.now + .seconds(Self.saveTimeout)
            while ContinuousClock.now < deadline {
                try? await Task.sleep(for: .seconds(Self.savePollInterval))
                guard let self, !Task.isCancelled else { return }
                self.app.refreshStatusNow()
                if self.app.profiles.contains(where: { $0.id == id }) {
                    self.app.closeHearingTest()
                    return
                }
            }
            guard let self, !Task.isCancelled else { return }
            self.phase = .saveFailed
        }
    }

    var defaultName: String {
        HearingTestRecord.defaultName(deviceName: device?.name ?? output?.name ?? "", date: testDate)
    }

    /// Screenshot hook (`AUDIOCURVE_DEMO=hearingtest`): shows a made-up state. No checks, tones or
    /// volume changes run.
    func showDemo(device: OutputDevice, date: Date, phase: Phase, result: HearingTestResult?) {
        checkTask?.cancel()
        checkTask = nil
        othersPlaying = false
        muted = false
        volumeSettable = true
        lowSampleRate = false
        self.device = device
        testDate = date
        self.result = result
        self.phase = phase
    }

    /// Stops everything; for leaving the test view and for quitting.
    func shutDown() {
        checkTask?.cancel()
        checkTask = nil
        saveTask?.cancel()
        saveTask = nil
        endActive()
    }

    /// Stops the test when the output, the other hosts, the sample rate or the generator changed;
    /// pauses it when another app plays. On the intro screen and while paused, refreshes the checks.
    func checkEnvironment() {
        switch phase {
        case .intro:
            refreshIntroChecks()
        case .running, .paused:
            guard let device else { return }
            if app.currentOutput?.uid != device.uid { stop(.outputChanged); return }
            if !app.otherHosts.isEmpty { stop(.otherHost); return }
            if Self.isLowSampleRate(uid: device.uid) { stop(.lowSampleRate); return }
            if case .running = phase {
                if generator?.isRunning != true { stop(.generatorStopped); return }
                if HAL.othersPlaying() { pause(.otherSound) }
            } else {
                refreshIntroChecks()
            }
        default:
            break
        }
    }

    // MARK: Running

    private func run() {
        runTask?.cancel()
        runTask = Task { [weak self] in await self?.runLoop() }
    }

    /// Per presentation: the gap, the tone, then the response window. A press ends the window
    /// early; no press is "not heard".
    private func runLoop() async {
        while !Task.isCancelled {
            guard let step = test?.next() else { return }
            let p: Presentation
            switch step {
            case .finished(let r):
                finish(r)
                return
            case .present(let next):
                p = next
            }
            phase = .running(p)
            guard await wait(p.gapBeforeS) else { return }
            guard let g = generator, g.isRunning else {
                stop(.generatorStopped)
                return
            }
            heard = false
            windowOpen = true
            g.play(ear: p.ear, frequencyHz: p.frequencyHz, levelDBFS: p.levelDBFS)
            // The tone (bounded in case the generator never reports it finished), then the window.
            let toneEnd = ContinuousClock.now + .seconds(ToneSynth.duration + 0.5)
            while g.isPlaying, !heard, ContinuousClock.now < toneEnd {
                guard await wait(Self.tick) else { return }
            }
            let windowEnd = ContinuousClock.now + .seconds(Self.responseWindow)
            while !heard, ContinuousClock.now < windowEnd {
                guard await wait(Self.tick) else { return }
            }
            windowOpen = false
            test?.respond(heard: heard)
        }
    }

    /// Sleeps; false when the task was cancelled.
    private func wait(_ seconds: TimeInterval) async -> Bool {
        do {
            try await Task.sleep(for: .seconds(seconds))
            return true
        } catch {
            return false
        }
    }

    /// Tones off, then the volume back at once (it is raised only while the test runs); resume
    /// raises it again. The unanswered presentation is played again on resume.
    private func pause(_ reason: PauseReason) {
        guard case .running = phase else { return }
        runTask?.cancel()
        runTask = nil
        windowOpen = false
        generator?.stop()
        restoreVolume()
        phase = .paused(reason)
        refreshIntroChecks()
    }

    private func finish(_ r: HearingTestResult) {
        endActive()
        test = nil
        result = r
        phase = .done(r)
    }

    /// Ends a running or paused test and goes back to the intro with `reason` (nil: cancelled).
    private func stop(_ reason: StopReason?) {
        guard isActive else { return }
        endActive()
        test = nil
        phase = .intro
        stopReason = reason
        refreshIntroChecks()
    }

    /// Tones off, volume back, route resumed. Safe to call more than once.
    private func endActive() {
        runTask?.cancel()
        runTask = nil
        windowOpen = false
        generator?.stop()
        generator = nil
        restoreVolume()
        if manualVolume { remindVolumeDown = true }
        app.testing = false
    }

    // MARK: Volume

    /// Saves the volume and raises it to max, with the generator running silent. A device without
    /// a settable volume passes (the intro asks the user to set it to max). False if the generator
    /// is not running silent or the volume could not be raised (then it is restored).
    ///
    /// A snapshot still pending for the device (a restore that failed at pause, or an earlier test)
    /// is the original: it is adopted, never replaced by the volume read now, which may be max.
    private func raiseVolume() -> Bool {
        guard let device, let g = generator, g.isRunning, !g.isPlaying else { return false }
        if let now = DeviceVolume.read(uid: device.uid) { VolumeSnapshot.savePending(now) }
        guard let s = VolumeSnapshot.pending(uid: device.uid) else {
            manualVolume = true
            return true
        }
        snapshot = s
        if DeviceVolume.setMax(s) { return true }
        restoreVolume()
        return false
    }

    /// Gives the volume back. What could not be restored stays pending and is retried
    /// (`AppModel.restorePendingVolumes`) until it is done or given up.
    private func restoreVolume() {
        guard let s = snapshot else { return }
        snapshot = nil
        VolumeSnapshot.updatePending(uid: s.deviceUID, remaining: DeviceVolume.restore(s))
    }

    private static func isLowSampleRate(uid: String) -> Bool {
        (HAL.nominalSampleRate(uid: uid) ?? .infinity) < ToneGenerator.minimumSampleRate
    }

    private func refreshIntroChecks() {
        guard let uid = output?.uid else {
            set(\.othersPlaying, false); set(\.muted, false); set(\.volumeSettable, true)
            set(\.lowSampleRate, false)
            return
        }
        set(\.othersPlaying, HAL.othersPlaying())
        set(\.muted, DeviceVolume.isMuted(uid: uid))
        set(\.volumeSettable, DeviceVolume.read(uid: uid) != nil)
        set(\.lowSampleRate, Self.isLowSampleRate(uid: uid))
    }

    /// Writes only changes, so the view is not invalidated every second.
    private func set(_ key: ReferenceWritableKeyPath<HearingTestModel, Bool>, _ value: Bool) {
        if self[keyPath: key] != value { self[keyPath: key] = value }
    }
}
