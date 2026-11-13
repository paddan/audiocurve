import SwiftUI
import AudioToolbox
import AudioCurveCore
import CAudioCurveDSP

enum Pane: String, CaseIterable, Identifiable {
    case audiogram, gain, spectrum
    var id: String { rawValue }
}

enum SpectrumEars { case left, right, both }

enum AutoLevelUIState: Equatable {
    case idle
    case measuring(remaining: Int)
    case done(Double)
    case tooQuiet
}

struct TrimSelection: Equatable {
    var ear: Ear
    var band: Int
}

/// Repeating main-run-loop timer that stops itself when released, so it never outlives its owner.
final class Ticker: @unchecked Sendable {
    private var timer: Timer?

    @MainActor
    func start(interval: TimeInterval, _ fire: @escaping @Sendable () -> Void) {
        stop()
        let t = Timer(timeInterval: interval, repeats: true) { _ in fire() }
        RunLoop.main.add(t, forMode: .common)   // keeps firing while the mouse is dragging
        timer = t
    }

    func stop() { timer?.invalidate(); timer = nil }
    deinit { timer?.invalidate() }
}

/// Registers a parameter-tree observer and removes it again when released.
final class ParamObserverBox: @unchecked Sendable {
    private let tree: AUParameterTree
    let token: AUParameterObserverToken

    init(tree: AUParameterTree, handler: @escaping @Sendable (AUParameterAddress, AUValue) -> Void) {
        self.tree = tree
        token = tree.token(byAddingParameterObserver: handler)
    }

    deinit { tree.removeParameterObserver(token) }
}

@Observable @MainActor
final class PluginViewModel {
    @ObservationIgnored let audioUnit: AudioCurveAudioUnit

    var profile: HearingProfile?
    var savedProfiles: [SavedProfile] = []
    /// Mirrors `audioUnit.sourceID`: the saved version the profile belongs to.
    var sourceID: UUID?
    var selection: PointSelection?
    var trimSelection: TrimSelection?
    var activeEar: Ear = .left
    var view: Pane = .audiogram
    var strength: Double = 50
    var tone: Double = 0
    var mpo: Double = -3
    var outputGain: Double = 0
    var inputGain: Double = 0
    var mode: CompressionMode = .auto
    var compressionOn = true
    var limiterOn = true
    var meters: ac_meters = PluginViewModel.silentMeters
    var spectrum: ac_spectrum?
    var spectrumEars: SpectrumEars = .both
    var autoLevelState: AutoLevelUIState = .idle
    var errorMessage: String?
    /// System output device right now, all output devices, and where processing is enabled.
    var currentOutput: OutputDevice?
    var availableOutputs: [OutputDevice] = []
    var activeOutputs = ActiveOutputs()
    var processingActive = true
    /// Digits typed for the selected point, shown in its label until Enter or Esc.
    var typedBuffer: String = ""

    @ObservationIgnored private let ticker = Ticker()
    @ObservationIgnored private var paramObserver: ParamObserverBox?
    @ObservationIgnored private var isPushing = false
    @ObservationIgnored private var errorTask: Task<Void, Never>?
    @ObservationIgnored private var autoLevelTask: Task<Void, Never>?

    private enum DragTarget {
        case point(PointSelection, offsetY: CGFloat)
        case trim(TrimSelection, startY: CGFloat, startValue: Double, range: ClosedRange<Double>)
    }
    @ObservationIgnored private var dragTarget: DragTarget?
    @ObservationIgnored private var dragStart: CGPoint = .zero
    @ObservationIgnored private var dragMoved = false
    /// Gain axis range frozen while a trim drag is running so the scale does not shift under the pointer.
    private var frozenGainRange: ClosedRange<Double>?

    static let silentMeters: ac_meters = {
        var m = ac_meters()
        m.in_rms_db = (-120, -120); m.in_peak_db = (-120, -120)
        m.out_rms_db = (-120, -120); m.out_peak_db = (-120, -120)
        m.reduction_db = (0, 0)
        m.speech = 0.5
        return m
    }()

    init(audioUnit: AudioCurveAudioUnit) {
        self.audioUnit = audioUnit
        #if DEBUG
        // Screenshot/demo hook: `defaults write <extension prefs> demoPane spectrum`.
        if let raw = UserDefaults.standard.string(forKey: "demoPane"), let pane = Pane(rawValue: raw) { view = pane }
        #endif
        profile = audioUnit.profile
        sourceID = audioUnit.sourceID
        savedProfiles = audioUnit.store.list()
        if let tree = audioUnit.parameterTree {
            func value(_ a: ParamAddress) -> Double? { tree.parameter(withAddress: a.rawValue).map { Double($0.value) } }
            strength = value(.strength) ?? strength
            tone = value(.tone) ?? tone
            mpo = value(.mpo) ?? mpo
            outputGain = value(.outputGain) ?? outputGain
            inputGain = value(.inputGain) ?? inputGain
            mode = CompressionMode(rawValue: Int((value(.mode) ?? 2).rounded())) ?? .auto
            compressionOn = (value(.compression) ?? 1) >= 0.5
            limiterOn = (value(.limiter) ?? 1) >= 0.5
            paramObserver = ParamObserverBox(tree: tree) { [weak self] address, value in
                Task { @MainActor in self?.applyHostParameter(address, Double(value)) }
            }
        }
        refreshOutputs()
        audioUnit.onOutputsChange = { [weak self] in self?.refreshOutputs() }
        audioUnit.onProfileChange = { [weak self] newProfile in
            guard let self, !self.isPushing else { return }
            self.profile = newProfile
            self.sourceID = self.audioUnit.sourceID   // a host may have loaded a saved profile
            self.reconcileSelection()
        }
    }

    // MARK: Outputs

    /// Re-reads the devices and the shared selection (another plugin instance may have changed it).
    func refreshOutputs() {
        audioUnit.reloadActiveOutputs()
        currentOutput = audioUnit.currentOutput
        availableOutputs = audioUnit.availableOutputs
        activeOutputs = audioUnit.activeOutputs
        processingActive = audioUnit.isProcessingActive
    }

    func setOutput(_ d: OutputDevice, enabled: Bool) {
        var a = activeOutputs
        a.setEnabled(d, enabled, available: availableOutputs)
        audioUnit.setActiveOutputs(a)
    }

    // MARK: Meters

    func startMeters() {
        ticker.start(interval: 0.05) { [weak self] in
            MainActor.assumeIsolated {
                guard let self else { return }
                self.meters = self.audioUnit.readMeters()
                if self.view == .spectrum { self.spectrum = self.audioUnit.readSpectrum() }
            }
        }
    }

    func stopMeters() { ticker.stop() }

    // MARK: Parameters

    private func applyHostParameter(_ address: AUParameterAddress, _ value: Double) {
        guard let a = ParamAddress(rawValue: address) else { return }
        switch a {
        case .strength: strength = value
        case .tone: tone = value
        case .mpo: mpo = value
        case .outputGain: outputGain = value
        case .mode: mode = CompressionMode(rawValue: Int(value.rounded())) ?? .auto
        case .compression: compressionOn = value >= 0.5
        case .limiter: limiterOn = value >= 0.5
        case .inputGain: inputGain = value
        }
    }

    func set(_ a: ParamAddress, _ value: Double) {
        switch a {
        case .strength: strength = value
        case .tone: tone = value
        case .mpo: mpo = value
        case .outputGain: outputGain = value
        case .mode: mode = CompressionMode(rawValue: Int(value.rounded())) ?? .auto
        case .compression: compressionOn = value >= 0.5
        case .limiter: limiterOn = value >= 0.5
        case .inputGain: inputGain = value
        }
        audioUnit.setParameter(a, AUValue(value), originator: paramObserver?.token)
    }

    func setMode(_ m: CompressionMode) { set(.mode, Double(m.rawValue)) }

    // MARK: Profile

    func addFlatProfile() {
        selection = nil; trimSelection = nil; typedBuffer = ""
        commit(.flat())
    }

    // MARK: Saved versions

    var currentName: String? {
        guard let id = sourceID else { return nil }
        return savedProfiles.first { $0.id == id }?.name
    }

    var isDirty: Bool {
        guard let p = profile else { return false }
        guard let id = sourceID, let saved = savedProfiles.first(where: { $0.id == id }) else { return true }
        return saved.profile != p
    }

    func refreshSaved() {
        savedProfiles = audioUnit.store.list()
        sourceID = audioUnit.sourceID
    }

    private func setSource(_ id: UUID?) {
        audioUnit.setSource(id)
        sourceID = id
    }

    func load(_ id: UUID) {
        refreshSaved()
        selection = nil; trimSelection = nil; typedBuffer = ""
        isPushing = true
        let ok = audioUnit.loadSaved(id: id)
        isPushing = false
        guard ok else { showError(String(localized: "Could not load the profile")); return }
        profile = audioUnit.profile
        sourceID = id
    }

    /// Overwrites the current version. Returns false when there is none, so the UI can ask for a name.
    @discardableResult
    func save() -> Bool {
        refreshSaved()
        guard let id = sourceID, savedProfiles.contains(where: { $0.id == id }) else { return false }
        guard let p = profile else { return true }
        perform { () throws(ProfileStoreError) in try audioUnit.store.overwrite(id: id, profile: p) }
        return true
    }

    func saveAs(_ name: String) {
        guard let p = profile else { return }
        perform { () throws(ProfileStoreError) in
            let saved = try audioUnit.store.saveNew(name: name, profile: p)
            setSource(saved.id)
        }
    }

    func rename(_ id: UUID, to name: String) {
        perform { () throws(ProfileStoreError) in try audioUnit.store.rename(id: id, to: name) }
    }

    func duplicate(_ id: UUID) {
        perform { () throws(ProfileStoreError) in _ = try audioUnit.store.duplicate(id: id, copyName: Fmt.copyName) }
    }

    func delete(_ id: UUID) {
        perform { () throws(ProfileStoreError) in
            try audioUnit.store.delete(id: id)
            if sourceID == id { setSource(nil) }
        }
    }

    private func perform(_ action: () throws(ProfileStoreError) -> Void) {
        do { try action() } catch {
            switch error {
            case .emptyName: showError(String(localized: "Enter a name"))
            case .nameTaken: showError(String(localized: "A version with that name already exists"))
            case .notFound, .io: showError(String(localized: "Could not save the profile"))
            }
        }
        refreshSaved()
    }

    private func commit(_ p: HearingProfile) {
        guard p != profile else { return }
        guard (try? p.validate()) != nil else { showError(String(localized: "Invalid profile")); return }
        profile = p
        isPushing = true
        audioUnit.profile = p
        isPushing = false
    }

    private func reconcileSelection() {
        guard let p = profile else { selection = nil; trimSelection = nil; typedBuffer = ""; return }
        if let s = selection, !p.audiogram(s.ear).points.contains(where: { $0.frequencyHz == s.frequencyHz }) {
            selection = nil; typedBuffer = ""
        }
    }

    func showError(_ text: String) {
        errorMessage = text
        errorTask?.cancel()
        errorTask = Task { [weak self] in
            try? await Task.sleep(for: .seconds(3))
            guard !Task.isCancelled else { return }
            self?.errorMessage = nil
        }
    }

    // MARK: Derived data for the views

    /// Both prescriptions for one ear (speech and music), from the same profile and settings.
    func fittings(_ ear: Ear) -> (speech: [BandFitting], music: [BandFitting]) {
        guard let p = profile else { return ([], []) }
        let s = strength / 100, t = tone / 100
        return (FittingRule.bands(for: p, ear: ear, strength: s, tone: t, prescription: .speech),
                FittingRule.bands(for: p, ear: ear, strength: s, tone: t, prescription: .music))
    }

    /// The fitting in effect: the chosen prescription, or in Auto the classifier-weighted blend.
    func fitting(_ ear: Ear) -> [BandFitting] {
        let f = fittings(ear)
        switch mode {
        case .speech: return f.speech
        case .music: return f.music
        case .auto: return FittingRule.blend(f.speech, f.music, weight: Double(meters.speech))
        }
    }

    /// Axis range (dB) for the gain chart. Covers both prescriptions; the blend in Auto always lies
    /// between them, so the range does not move with the classifier.
    var gainRange: ClosedRange<Double> {
        if let f = frozenGainRange { return f }
        var lo = 0.0, hi = 40.0
        for ear in Ear.allCases {
            let f = fittings(ear)
            for b in f.speech + f.music {
                hi = max(hi, b.gainSoftDB, b.gainLoudDB)
                lo = min(lo, b.gainSoftDB, b.gainLoudDB)
            }
        }
        return (((lo - 5) / 10).rounded(.down) * 10 ... ((hi + 5) / 10).rounded(.up) * 10)
    }

    func label(for s: PointSelection) -> String {
        guard let p = profile, let pt = p.audiogram(s.ear).points.first(where: { $0.frequencyHz == s.frequencyHz })
        else { return "" }
        let value = typedBuffer.isEmpty ? Fmt.db(pt.thresholdDBHL) : typedBuffer + "_"
        return "\(Fmt.earInitial(s.ear)) \(Fmt.freq(s.frequencyHz)) · \(value) dB HL"   // units only, nothing to translate
    }

    func label(for t: TrimSelection) -> String {
        guard let p = profile else { return "" }
        let trim = p.trim(t.ear)[t.band]
        let gain = fitting(t.ear)[t.band].gainSoftDB
        return String(localized: "\(Fmt.earInitial(t.ear)) \(Fmt.freq(Bands.centers[t.band])) · trim \(Fmt.signed(trim)) dB · gain \(Fmt.db(gain)) dB",
                      comment: "Gain bar label: ear initial, band frequency, trim, resulting gain")
    }

    func accessibilityAdjust(_ s: PointSelection, up: Bool) {
        guard var p = profile else { return }
        selection = s; activeEar = s.ear
        AudiogramEditing.nudge(&p, s, up: up, fine: false)
        commit(p)
    }

    func accessibilityAdjustTrim(_ t: TrimSelection, up: Bool) {
        guard var p = profile else { return }
        trimSelection = t; activeEar = t.ear
        AudiogramEditing.nudgeTrim(&p, ear: t.ear, band: t.band, delta: up ? 1 : -1)
        commit(p)
    }

    // MARK: Keyboard

    private func optionDown(_ key: KeyPress) -> Bool {
        key.modifiers.contains(.option) || NSEvent.modifierFlags.contains(.option)
    }

    func handleKey(_ key: KeyPress) -> KeyPress.Result {
        guard profile != nil, view != .spectrum else { return .ignored }
        if key.modifiers.contains(.command) || key.modifiers.contains(.control) { return .ignored }
        return view == .audiogram ? handleAudiogramKey(key) : handleGainKey(key)
    }

    private func handleAudiogramKey(_ key: KeyPress) -> KeyPress.Result {
        guard var p = profile else { return .ignored }
        let fine = optionDown(key)
        let points = p.audiogram(activeEar).points
        switch key.key {
        case .escape:
            selection = nil; typedBuffer = ""
            return .handled
        case .upArrow, .downArrow:
            guard let s = selection else { selectFirst(.audiogram); return .handled }
            typedBuffer = ""
            AudiogramEditing.nudge(&p, s, up: key.key == .upArrow, fine: fine)
            commit(p)
            return .handled
        case .leftArrow, .rightArrow:
            guard let s = selection else {
                selection = key.key == .leftArrow ? points.last.map { PointSelection(ear: activeEar, frequencyHz: $0.frequencyHz) }
                                                  : points.first.map { PointSelection(ear: activeEar, frequencyHz: $0.frequencyHz) }
                return .handled
            }
            typedBuffer = ""
            if let n = AudiogramEditing.neighbor(p, of: s, forward: key.key == .rightArrow) { selection = n }
            return .handled
        case .tab:
            typedBuffer = ""
            if let s = selection {
                let n = AudiogramEditing.switchEar(p, s)
                selection = n; activeEar = n.ear
            } else {
                activeEar = activeEar == .left ? .right : .left
                selectFirst(.audiogram)
            }
            return .handled
        case .delete, .deleteForward:
            if !typedBuffer.isEmpty { typedBuffer.removeLast(); return .handled }
            guard let s = selection else { return .handled }
            let next = AudiogramEditing.neighbor(p, of: s, forward: false) ?? AudiogramEditing.neighbor(p, of: s, forward: true)
            if AudiogramEditing.delete(&p, s) {
                selection = next
                commit(p)
            } else {
                showError(String(localized: "At least \(AudiogramEditing.minPoints) points per ear"))
            }
            return .handled
        case .return:
            return commitTyped(&p)
        default:
            if key.characters == "\u{3}" { return commitTyped(&p) }   // keypad Enter
            guard selection != nil else { return .ignored }
            for ch in key.characters {
                if ch.isASCII, ch.isNumber { if typedBuffer.count < 4 { typedBuffer.append(ch) } }
                else if ch == "-" || ch == "\u{2212}" { if typedBuffer.isEmpty { typedBuffer = "-" } }
                else { return .ignored }
            }
            return .handled
        }
    }

    private func commitTyped(_ p: inout HearingProfile) -> KeyPress.Result {
        guard !typedBuffer.isEmpty, let s = selection else { return .handled }
        if let v = AudiogramEditing.parseTyped(typedBuffer) {
            AudiogramEditing.setValue(&p, s, dbHL: v, fine: true)
            commit(p)
        } else {
            showError(String(localized: "Enter a value between −10 and 120 dB HL"))
        }
        typedBuffer = ""
        return .handled
    }

    private func selectFirst(_ pane: Pane) {
        guard let p = profile else { return }
        if pane == .audiogram {
            selection = p.audiogram(activeEar).points.first.map { PointSelection(ear: activeEar, frequencyHz: $0.frequencyHz) }
        } else {
            trimSelection = TrimSelection(ear: activeEar, band: 0)
        }
    }

    private func handleGainKey(_ key: KeyPress) -> KeyPress.Result {
        guard var p = profile else { return .ignored }
        switch key.key {
        case .escape:
            trimSelection = nil
        case .upArrow, .downArrow:
            guard let t = trimSelection else { selectFirst(.gain); return .handled }
            AudiogramEditing.nudgeTrim(&p, ear: t.ear, band: t.band, delta: key.key == .upArrow ? 1 : -1)
            commit(p)
        case .leftArrow, .rightArrow:
            guard let t = trimSelection else { selectFirst(.gain); return .handled }
            let nb = min(Bands.count - 1, max(0, t.band + (key.key == .rightArrow ? 1 : -1)))
            trimSelection = TrimSelection(ear: t.ear, band: nb)
        case .tab:
            let ear: Ear = (trimSelection?.ear ?? activeEar) == .left ? .right : .left
            trimSelection = TrimSelection(ear: ear, band: trimSelection?.band ?? 0)
            activeEar = ear
        case .delete, .deleteForward:
            guard let t = trimSelection else { return .handled }
            AudiogramEditing.resetTrim(&p, ear: t.ear, band: t.band)
            commit(p)
        default:
            return .ignored
        }
        return .handled
    }

    // MARK: Pointer

    static let hitRadius: CGFloat = 12

    func beginDrag(at point: CGPoint, in size: CGSize) {
        dragTarget = nil; dragStart = point; dragMoved = false
        guard let p = profile else { return }
        typedBuffer = ""
        if view == .audiogram {
            beginPointDrag(p, at: point, size: size)
        } else {
            beginTrimDrag(p, at: point, size: size)
        }
    }

    private func beginPointDrag(_ p: HearingProfile, at pt: CGPoint, size: CGSize) {
        let g = PlotGeometry(size: size)
        for ear in [activeEar, activeEar == .left ? Ear.right : .left] {
            let nearest = p.audiogram(ear).points
                .map { ($0, hypot(g.x($0.frequencyHz) - pt.x, g.y($0.thresholdDBHL) - pt.y)) }
                .filter { $0.1 <= Self.hitRadius }
                .min { $0.1 < $1.1 }
            if let (hit, _) = nearest {
                let sel = PointSelection(ear: ear, frequencyHz: hit.frequencyHz)
                selection = sel; activeEar = ear
                dragTarget = .point(sel, offsetY: g.y(hit.thresholdDBHL) - pt.y)
                return
            }
        }
        guard g.plot.insetBy(dx: -Self.hitRadius, dy: -Self.hitRadius).contains(pt),
              let f = Audiogram.standardFrequencies
                .map({ ($0, abs(g.x($0) - pt.x)) }).filter({ $0.1 <= Self.hitRadius }).min(by: { $0.1 < $1.1 })?.0
        else { selection = nil; return }
        let sel = PointSelection(ear: activeEar, frequencyHz: f)
        selection = sel
        var q = p
        if let existing = p.audiogram(activeEar).points.first(where: { $0.frequencyHz == f }) {
            dragTarget = .point(sel, offsetY: g.y(existing.thresholdDBHL) - pt.y)
        } else {
            let v = AudiogramEditing.dbHL(atY: pt.y - g.plot.minY, plotHeight: g.plot.height)
            AudiogramEditing.setValue(&q, sel, dbHL: v, fine: NSEvent.modifierFlags.contains(.option))
            commit(q)
            dragTarget = .point(sel, offsetY: 0)
            dragMoved = true
        }
    }

    private func beginTrimDrag(_ p: HearingProfile, at pt: CGPoint, size: CGSize) {
        let range = gainRange
        let g = GainGeometry(size: size, range: range)
        guard let t = g.hit(pt) else { trimSelection = nil; return }
        trimSelection = t; activeEar = t.ear
        frozenGainRange = range
        dragTarget = .trim(t, startY: pt.y, startValue: p.trim(t.ear)[t.band], range: range)
    }

    func drag(to point: CGPoint, in size: CGSize, fine: Bool) {
        guard let target = dragTarget, var p = profile else { return }
        if !dragMoved {
            guard hypot(point.x - dragStart.x, point.y - dragStart.y) >= 2 else { return }
            dragMoved = true
        }
        switch target {
        case let .point(sel, offsetY):
            let g = PlotGeometry(size: size)
            let v = AudiogramEditing.dbHL(atY: point.y + offsetY - g.plot.minY, plotHeight: g.plot.height)
            AudiogramEditing.setValue(&p, sel, dbHL: v, fine: fine)
        case let .trim(t, startY, startValue, range):
            let g = GainGeometry(size: size, range: range)
            let perPoint = (range.upperBound - range.lowerBound) / g.plot.height
            let target = min(20, max(-20, (startValue + (startY - point.y) * perPoint).rounded()))
            AudiogramEditing.nudgeTrim(&p, ear: t.ear, band: t.band, delta: target - p.trim(t.ear)[t.band])
        }
        commit(p)
    }

    func endDrag() {
        dragTarget = nil
        frozenGainRange = nil
    }

    /// Double click in the gain chart: reset that bar's trim.
    func resetTrim(at point: CGPoint, in size: CGSize) {
        guard view == .gain, var p = profile,
              let t = GainGeometry(size: size, range: gainRange).hit(point) else { return }
        trimSelection = t; activeEar = t.ear
        AudiogramEditing.resetTrim(&p, ear: t.ear, band: t.band)
        commit(p)
    }

    // MARK: Auto level

    func startAutoLevel() {
        if case .measuring = autoLevelState { return }
        autoLevelState = .measuring(remaining: 3)
        autoLevelTask?.cancel()
        autoLevelTask = Task { [weak self] in
            var m = AutoLevelMeasurement()
            let blocks = 60                         // 3 s at 50 ms
            for i in 0..<blocks {
                guard let self, !Task.isCancelled else { return }
                m.add(self.audioUnit.readMeters())
                self.autoLevelState = .measuring(remaining: Int((Double(blocks - i) * 0.05).rounded(.up)))
                try? await Task.sleep(for: .milliseconds(50))
            }
            guard let self, !Task.isCancelled else { return }
            switch m.result(currentOutputGainDB: self.outputGain, mpoDBFS: self.mpo) {
            case .tooQuiet:
                self.autoLevelState = .tooQuiet
            case .gain(let g):
                let rounded = (g * 10).rounded() / 10
                self.set(.outputGain, rounded)
                self.autoLevelState = .done(rounded)
            }
        }
    }
}

// MARK: - Formatting

enum Fmt {
    /// Decimal separator of the user's region (comma in most of the supported locales).
    private static var decimal: String { Locale.current.decimalSeparator ?? "." }

    static func freq(_ hz: Double) -> String {
        if hz < 1000 { return "\(Int(hz)) Hz" }
        let k = hz / 1000
        if k == k.rounded() { return String(format: "%.0f kHz", k) }
        return String(format: "%.1f kHz", k).replacingOccurrences(of: ".", with: decimal)
    }

    static func axisFreq(_ hz: Double) -> String {
        if hz < 1000 { return "\(Int(hz))" }
        let k = hz / 1000
        return (k == k.rounded() ? String(format: "%.0fk", k) : String(format: "%.1fk", k))
            .replacingOccurrences(of: ".", with: decimal)
    }

    /// Whole numbers without decimals, otherwise one decimal; regional decimal separator and true minus sign.
    static func db(_ v: Double) -> String {
        let r = (v * 10).rounded() / 10
        let s = r == r.rounded() ? String(format: "%.0f", abs(r)) : String(format: "%.1f", abs(r)).replacingOccurrences(of: ".", with: decimal)
        return r < 0 ? "\u{2212}" + s : s
    }

    static func signed(_ v: Double) -> String {
        let s = db(v)
        return v > 0 ? "+" + s : s
    }

    /// "50 %" or "50%" depending on the language.
    static func percent(_ v: Double) -> String {
        String(localized: "\(Int(v.rounded())) %", comment: "Percentage; keep or drop the space per language")
    }

    static func earInitial(_ e: Ear) -> String {
        e == .left ? String(localized: "L", comment: "Initial of the left ear") : String(localized: "R", comment: "Initial of the right ear")
    }

    static func earName(_ e: Ear) -> String {
        e == .left ? String(localized: "Left", comment: "Left ear") : String(localized: "Right", comment: "Right ear")
    }

    /// Name for a duplicated profile: "<name> copy", then "<name> copy 2", …
    static func copyName(_ base: String, _ n: Int) -> String {
        n == 1 ? String(localized: "\(base) copy", comment: "Name of a duplicated profile")
               : String(localized: "\(base) copy \(n)", comment: "Name of the nth duplicate of a profile")
    }
}
