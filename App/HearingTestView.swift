import AudioCurveCore
import SwiftUI

/// The hearing test in the main window: intro, the running test and the result (spec 4.2).
@MainActor
struct HearingTestView: View {
    let model: HearingTestModel

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            HStack {
                Text("Hearing test").font(.title2.bold())
                Spacer()
                if let name = (model.device ?? model.output)?.name {
                    Text("Output: \(name)").foregroundStyle(.secondary)
                }
            }
            switch model.phase {
            case .intro:
                IntroView(model: model)
            case .running(let p):
                RunningView(model: model, presentation: p)
            case .paused(let reason):
                PausedView(model: model, reason: reason)
            case .done, .saving, .saveFailed:
                if let result = model.result { ResultView(model: model, result: result) }
            }
            Spacer(minLength: 0)
        }
        .padding(8)
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
    }
}

// MARK: - Intro

private struct IntroView: View {
    @Bindable var model: HearingTestModel

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            Text("Before you start").font(.headline)
            VStack(alignment: .leading, spacing: 6) {
                bullet("Sit somewhere quiet. Turn on noise cancellation if your headphones have it.")
                bullet("Stop other sound on the Mac.")
                bullet("Turn on Do Not Disturb so alerts don't play during the test.")
                bullet("Turn off any system-wide EQ or sound enhancer.")
                if model.volumeSettable {
                    bullet("The test sets the headphone volume to max and controls the level itself. The volume is restored afterwards.")
                } else {
                    bullet("The test controls the level itself. Turn the volume back down when it ends.")
                }
            }
            Picker("Headphones", selection: transducer) {
                Text("In-ear").tag(Transducer.inEar)
                Text("Over-ear").tag(Transducer.overEar)
            }
            .pickerStyle(.segmented)
            .fixedSize()

            VStack(alignment: .leading, spacing: 6) {
                if let reason = model.stopReason { Warning(text: stopText(reason)) }
                if model.remindVolumeDown { Warning(text: turnVolumeDownText) }
                if let host = model.blockingHostName {
                    Warning(text: String(localized: "AudioCurve is running in \(host). Quit it to start the test.",
                                         comment: "Hearing test intro: another AudioCurve instance blocks the test"))
                }
                if model.muted {
                    Warning(text: String(localized: "The output is muted. Unmute it to start the test."))
                }
                if model.othersPlaying {
                    Warning(text: String(localized: "Another app is playing sound. Stop it before you start."))
                }
                if model.lowSampleRate { Warning(text: lowSampleRateText) }
                if !model.volumeSettable {
                    Warning(text: String(localized: "AudioCurve cannot set the volume of this output, so the level may be off. Set the volume to max yourself before you start."))
                }
            }

            HStack {
                Button("Start test") { model.start() }
                    .buttonStyle(.borderedProminent)
                    .keyboardShortcut(.defaultAction)
                    .disabled(!model.canStart)
                Button("Cancel") { model.cancel() }
            }
        }
    }

    private var transducer: Binding<Transducer> {
        Binding(get: { model.transducer }, set: { model.transducerChoice = $0 })
    }

    private func bullet(_ text: LocalizedStringKey) -> some View {
        HStack(alignment: .firstTextBaseline, spacing: 6) {
            Text(verbatim: "•")
            Text(text).fixedSize(horizontal: false, vertical: true)
        }
    }

    private func stopText(_ reason: HearingTestModel.StopReason) -> String {
        switch reason {
        case .outputChanged: String(localized: "The test stopped because the output changed.")
        case .otherHost: String(localized: "The test stopped because AudioCurve started in another app.")
        case .windowClosed: String(localized: "The test stopped because the window was closed.")
        case .generatorStopped: String(localized: "The test stopped because the output stopped playing.")
        case .generatorFailed: String(localized: "The test tones could not be played on this output.")
        case .volumeNotSet: String(localized: "The volume could not be set to max, so the test did not start.")
        case .lowSampleRate: String(localized: "The test stopped because the output switched to a low sample rate.")
        }
    }
}

// MARK: - Running

private struct RunningView: View {
    let model: HearingTestModel
    let presentation: Presentation
    @FocusState private var focused: Bool

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            HStack {
                // The ear is shown by the ear indicator below.
                Text(verbatim: "\(Int(presentation.frequencyHz).formatted()) Hz")
                    .font(.headline)
                Spacer()
                ProgressView(value: Double(presentation.index), total: Double(HearingTest.totalPoints))
                    .frame(width: 160)
                Text(verbatim: "\(presentation.index) / \(HearingTest.totalPoints)")
                    .monospacedDigit().foregroundStyle(.secondary)
            }
            Text("Press Space or click when you hear a tone.").foregroundStyle(.secondary)
            Spacer(minLength: 8)
            EarIndicator(active: presentation.ear)
                .frame(maxWidth: .infinity)
            Button { model.respond() } label: {
                Text("I hear it").font(.title2).frame(width: 240, height: 56)
            }
            .buttonStyle(.borderedProminent)
            .frame(maxWidth: .infinity)
            Spacer(minLength: 8)
            HStack {
                Spacer()
                Button("Pause") { model.pause() }
                Button("Cancel") { model.cancel() }
            }
        }
        .frame(maxHeight: .infinity, alignment: .top)
        .focusable()
        .focused($focused)
        .focusEffectDisabled()
        .onKeyPress(.space, phases: .down) { _ in   // .down only: auto-repeat is ignored
            model.respond()
            return .handled
        }
        .onAppear { focused = true }
    }
}

/// The two ears, seen from the front as the listener faces the screen: left ear (blue) on the left,
/// right ear (red) on the right, in the audiogram's colors. The ear being tested is lit and pulses.
struct EarIndicator: View {
    let active: Ear?

    var body: some View {
        HStack(spacing: 96) {
            ear(.left)
            ear(.right)
        }
        .accessibilityElement(children: .combine)
    }

    private func ear(_ e: Ear) -> some View {
        let on = e == active
        let color = e == .left ? AudiogramChart.left : AudiogramChart.right
        return VStack(spacing: 8) {
            Image(systemName: "ear.fill")
                .font(.system(size: 110))
                // Mirror the left one so both lobes face the middle, where the head would be.
                .scaleEffect(x: e == .left ? -1 : 1, y: 1)
                .foregroundStyle(on ? color : Color.secondary.opacity(0.25))
                .symbolEffect(.pulse, options: .repeating, isActive: on)
                .frame(width: 170, height: 170)
                .background(Circle().fill(color.opacity(on ? 0.14 : 0)))
            Text(earName(e))
                .font(.title3.weight(on ? .semibold : .regular))
                .foregroundStyle(on ? color : Color.secondary)
        }
        .animation(.easeInOut(duration: 0.3), value: on)
    }
}

// MARK: - Paused

private struct PausedView: View {
    let model: HearingTestModel
    let reason: HearingTestModel.PauseReason

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            Text("Paused").font(.headline)
            if reason == .otherSound || model.othersPlaying {
                Warning(text: String(localized: "Another app is playing sound. Stop it, then resume the test."))
            }
            if model.muted {
                Warning(text: String(localized: "The output is muted. Unmute it, then resume the test."))
            }
            if model.lowSampleRate { Warning(text: lowSampleRateText) }
            HStack {
                Button("Resume") { model.resume() }
                    .buttonStyle(.borderedProminent)
                    .keyboardShortcut(.defaultAction)
                    .disabled(!model.canResume)
                Button("Cancel") { model.cancel() }
            }
        }
    }
}

// MARK: - Result

private struct ResultView: View {
    let model: HearingTestModel
    let result: HearingTestResult
    @ViewState private var name: String
    @ViewState private var assign = true

    init(model: HearingTestModel, result: HearingTestResult) {
        self.model = model
        self.result = result
        _name = ViewState(initialValue: model.defaultName)
    }

    var body: some View {
        let deviceName = model.device?.name ?? ""
        VStack(alignment: .leading, spacing: 12) {
            HStack(alignment: .firstTextBaseline) {
                Text("Done").font(.headline)
                Text("Relative to \(deviceName)", comment: "Hearing test result: measured with this output")
                    .foregroundStyle(.secondary)
                Spacer()
                Legend()
            }
            AudiogramChart(left: result.left, right: result.right)
                .frame(height: 300)
            VStack(alignment: .leading, spacing: 6) {
                if model.remindVolumeDown { Warning(text: turnVolumeDownText) }
                ForEach(Array(TestWarning.from(result.flags).enumerated()), id: \.offset) { _, w in
                    Warning(text: Self.text(w))
                }
            }
            Text("After saving, play some audio and press Auto level to set the output level.")
                .foregroundStyle(.secondary)
            HStack {
                TextField("Name", text: $name).frame(maxWidth: 320)
                Toggle("Use on \(deviceName)", isOn: $assign)
            }
            .disabled(isSaving)
            HStack {
                switch model.phase {
                case .saving:
                    ProgressView().controlSize(.small)
                    Text("Saving…")
                case .saveFailed:
                    Warning(text: String(localized: "The profile could not be saved."))
                    Button("Retry") { model.save(name: name, assign: assign) }
                        .buttonStyle(.borderedProminent)
                    Button("Discard") { model.discard() }
                default:
                    Button("Save") { model.save(name: name, assign: assign) }
                        .buttonStyle(.borderedProminent)
                        .keyboardShortcut(.defaultAction)
                    Button("Discard") { model.discard() }
                }
            }
        }
    }

    private var isSaving: Bool {
        if case .saving = model.phase { return true }
        return false
    }

    static func text(_ w: TestWarning) -> String {
        switch w {
        case let .retestMismatch(ear, dB):
            let d = "\(Int(dB.rounded()))"
            return ear == .left
                ? String(localized: "The 1 kHz retest differed by \(d) dB in the left ear — consider testing again.")
                : String(localized: "The 1 kHz retest differed by \(d) dB in the right ear — consider testing again.")
        case .catchTrialPresses:
            return String(localized: "You may be pressing without hearing a tone — consider testing again.")
        case let .noResponseAtMax(ear, fs):
            let f = list(fs)
            return ear == .left
                ? String(localized: "No response at the maximum level at \(f) Hz (left ear): the real threshold may be higher.")
                : String(localized: "No response at the maximum level at \(f) Hz (right ear): the real threshold may be higher.")
        case let .notConverged(ear, fs):
            let f = list(fs)
            switch (ear, fs.count > 1) {
            case (.left, false): return String(localized: "The \(f) Hz result (left ear) is uncertain.")
            case (.right, false): return String(localized: "The \(f) Hz result (right ear) is uncertain.")
            case (.left, true): return String(localized: "The \(f) Hz results (left ear) are uncertain.")
            case (.right, true): return String(localized: "The \(f) Hz results (right ear) are uncertain.")
            }
        case .asymmetry:
            return String(localized: "The worse ear may be underestimated. An audiologist can measure it with masking.")
        }
    }

    private static func list(_ fs: [Double]) -> String { fs.map { "\(Int($0))" }.joined(separator: ", ") }
}

private struct Legend: View {
    var body: some View {
        HStack(spacing: 12) {
            HStack(spacing: 4) {
                Text(verbatim: "○").foregroundStyle(AudiogramChart.right)
                Text(earName(.right))
            }
            HStack(spacing: 4) {
                Text(verbatim: "×").foregroundStyle(AudiogramChart.left)
                Text(earName(.left))
            }
        }
        .font(.callout)
    }
}

// MARK: - Shared

/// After a test on an output whose volume the user raised to max by hand.
private var turnVolumeDownText: String {
    String(localized: "Turn the volume back down now.",
           comment: "Hearing test: after a test on an output whose volume AudioCurve cannot set")
}

private var lowSampleRateText: String {
    String(localized: "The output runs at a low sample rate (for example a headset in call mode). Stop using its microphone and try again.")
}

private func earName(_ ear: Ear) -> String {
    ear == .left ? String(localized: "Left ear") : String(localized: "Right ear")
}

private struct Warning: View {
    let text: String

    var body: some View {
        Label {
            Text(text).fixedSize(horizontal: false, vertical: true)
        } icon: {
            Image(systemName: "exclamationmark.triangle.fill").foregroundStyle(Color(nsColor: .systemOrange))
        }
    }
}

/// A read-only audiogram in the plugin editor's style: dB HL downwards (−10…120), log frequency
/// 125–8000 Hz, right ear red circles, left ear blue crosses.
struct AudiogramChart: View {
    static let left = Color(red: 0x37 / 255, green: 0x8A / 255, blue: 0xDD / 255)    // #378ADD, as the editor
    static let right = Color(red: 0xE2 / 255, green: 0x4A / 255, blue: 0x4A / 255)   // #E24B4A, as the editor
    private static let tick = Font.system(size: 12).monospacedDigit()
    private static let leftInset: CGFloat = 40, rightInset: CGFloat = 20, topInset: CGFloat = 16, bottomInset: CGFloat = 28

    let left: [TestPoint]
    let right: [TestPoint]

    var body: some View {
        Canvas { ctx, size in draw(ctx, size) }
            .accessibilityLabel(Text("Audiogram"))
    }

    private func draw(_ ctx: GraphicsContext, _ size: CGSize) {
        let plot = CGRect(x: Self.leftInset, y: Self.topInset,
                          width: max(1, size.width - Self.leftInset - Self.rightInset),
                          height: max(1, size.height - Self.topInset - Self.bottomInset))
        func x(_ hz: Double) -> CGFloat { plot.minX + CGFloat(log2(hz / 125) / 6) * plot.width }
        func y(_ db: Double) -> CGFloat { plot.minY + CGFloat((db + 10) / 130) * plot.height }
        let grid = Color.primary.opacity(0.08)

        // Normal-hearing band (−10…20 dB HL).
        ctx.fill(Path(CGRect(x: plot.minX, y: y(-10), width: plot.width, height: y(20) - y(-10))),
                 with: .color(Color.green.opacity(0.10)))

        for db in stride(from: -10.0, through: 120.0, by: 10.0) {
            var line = Path()
            line.move(to: CGPoint(x: plot.minX, y: y(db)))
            line.addLine(to: CGPoint(x: plot.maxX, y: y(db)))
            ctx.stroke(line, with: .color(db == 0 ? Color.primary.opacity(0.22) : grid), lineWidth: 1)
            if db >= 0 && Int(db) % 20 == 0 {
                ctx.draw(Text(verbatim: "\(Int(db))").font(Self.tick).foregroundStyle(.secondary),
                         at: CGPoint(x: plot.minX - 8, y: y(db)), anchor: .trailing)
            }
        }
        for f in Audiogram.standardFrequencies {
            var line = Path()
            line.move(to: CGPoint(x: x(f), y: plot.minY))
            line.addLine(to: CGPoint(x: x(f), y: plot.maxY))
            ctx.stroke(line, with: .color(grid), lineWidth: 1)
            ctx.draw(Text(verbatim: Self.axisLabel(f)).font(Self.tick).foregroundStyle(.secondary),
                     at: CGPoint(x: x(f), y: plot.maxY + 8), anchor: .top)
        }

        for (ear, points) in [(Ear.left, left), (Ear.right, right)] {
            let color = ear == .left ? Self.left : Self.right
            let pts = points.map { CGPoint(x: x($0.frequencyHz), y: y($0.hl)) }
            var path = Path()
            for (i, p) in pts.enumerated() { i == 0 ? path.move(to: p) : path.addLine(to: p) }
            ctx.stroke(path, with: .color(color.opacity(0.7)), style: StrokeStyle(lineWidth: 1.5, lineJoin: .round))
            for p in pts { marker(ctx, ear: ear, at: p, color: color) }
        }
    }

    private func marker(_ ctx: GraphicsContext, ear: Ear, at p: CGPoint, color: Color) {
        let r: CGFloat = 5.5
        if ear == .left {
            var x = Path()
            x.move(to: CGPoint(x: p.x - r, y: p.y - r)); x.addLine(to: CGPoint(x: p.x + r, y: p.y + r))
            x.move(to: CGPoint(x: p.x - r, y: p.y + r)); x.addLine(to: CGPoint(x: p.x + r, y: p.y - r))
            ctx.stroke(x, with: .color(color), style: StrokeStyle(lineWidth: 2, lineCap: .round))
        } else {
            ctx.stroke(Path(ellipseIn: CGRect(x: p.x - r, y: p.y - r, width: 2 * r, height: 2 * r)),
                       with: .color(color), lineWidth: 2)
        }
    }

    /// "125", "1k", "1.5k" with the region's decimal separator, as the editor.
    private static func axisLabel(_ hz: Double) -> String {
        if hz < 1000 { return "\(Int(hz))" }
        let k = hz / 1000
        let s = k == k.rounded() ? String(format: "%.0fk", k) : String(format: "%.1fk", k)
        return s.replacingOccurrences(of: ".", with: Locale.current.decimalSeparator ?? ".")
    }
}
