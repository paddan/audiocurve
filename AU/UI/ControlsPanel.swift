import SwiftUI
import AudioToolbox
import AudioCurveCore

struct ControlsPanel: View {
    @Bindable var vm: PluginViewModel
    @ViewState private var confirmLimiterOff = false

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Picker("Mode", selection: Binding(get: { vm.mode }, set: { vm.setMode($0) })) {
                Text("Speech").tag(CompressionMode.speech)
                Text("Music").tag(CompressionMode.music)
                Text("Auto", comment: "Compression mode: detect speech or music automatically").tag(CompressionMode.auto)
            }
            .pickerStyle(.segmented)
            .labelsHidden()
            if vm.mode == .auto {
                AutoModeStatus(vm: vm)
            }

            SliderRow(title: "Strength", value: binding(.strength), range: 0...100, step: 1, defaultValue: 50) {
                Fmt.percent($0)
            }
            SliderRow(title: "Tone", value: binding(.tone), range: 0...100, step: 1, defaultValue: 0,
                      minimumLabel: "Clear", maximumLabel: "Warm") {
                Fmt.percent($0)
            }
            SliderRow(title: "Input level", value: binding(.inputGain), range: -30...12, step: 0.5, defaultValue: 0) {
                "\(Fmt.signed($0)) dB"
            }
            .help(Text("Lowers or raises the sound before it is amplified. A lower input level makes the compression work less."))
            SliderRow(title: "Output level", value: binding(.outputGain), range: -48...12, step: 0.5, defaultValue: 0) {
                "\(Fmt.signed($0)) dB"
            }
            SliderRow(title: "Ceiling (MPO)", value: binding(.mpo), range: -20 ... -1, step: 1, defaultValue: -3) {
                "\(Fmt.db($0)) dBFS"
            }

            HStack(spacing: 16) {
                Toggle("Compression", isOn: Binding(get: { vm.compressionOn }, set: { vm.set(.compression, $0 ? 1 : 0) }))
                Toggle("Limiter", isOn: Binding(get: { vm.limiterOn }, set: { on in
                    // Turning the safety stage off needs a confirmation; turning it on never does.
                    if on { vm.set(.limiter, 1) } else { confirmLimiterOff = true }
                }))
            }
            .toggleStyle(.checkbox)
            .font(Theme.label)
            ActiveOutputsDropdown(vm: vm)
            if !vm.limiterOn {
                Text("Limiter off: no level ceiling below 0 dBFS")
                    .font(Theme.label)
                    .foregroundStyle(Color(nsColor: .systemOrange))
            }

            VStack(alignment: .leading, spacing: 4) {
                Button { vm.startAutoLevel() } label: {
                    Text("Auto level").frame(maxWidth: .infinity)
                }
                .controlSize(.large)
                .disabled(isMeasuring)
                Text(autoLevelText)
                    .font(Theme.label)
                    .foregroundStyle(vm.autoLevelState == .tooQuiet ? Color(nsColor: .systemOrange) : .secondary)
                    .lineLimit(1)
                    .minimumScaleFactor(0.85)
                    .frame(height: 16, alignment: .leading)
            }

            Spacer(minLength: 0)

            MetersBlock(vm: vm)
        }
        .padding(16)
        .frame(maxHeight: .infinity, alignment: .top)
        .alert("Turn off the limiter?", isPresented: $confirmLimiterOff) {
            Button("Turn Off", role: .destructive) { vm.set(.limiter, 0) }
            Button("Cancel", role: .cancel) {}
        } message: {
            Text("Without the limiter there is no level ceiling below 0 dBFS. With strong amplification the sound can get very loud and damage your hearing. Lower the volume before turning it off.")
        }
        .background(RoundedRectangle(cornerRadius: Theme.corner).fill(Color.primary.opacity(0.04)))
        .overlay(RoundedRectangle(cornerRadius: Theme.corner).strokeBorder(Color.primary.opacity(0.08), lineWidth: 1))
    }

    private func binding(_ a: ParamAddress) -> Binding<Double> {
        Binding(
            get: {
                switch a {
                case .strength: vm.strength
                case .tone: vm.tone
                case .mpo: vm.mpo
                case .outputGain: vm.outputGain
                case .mode: Double(vm.mode.rawValue)
                case .compression: vm.compressionOn ? 1 : 0
                case .limiter: vm.limiterOn ? 1 : 0
                case .inputGain: vm.inputGain
                }
            },
            set: { vm.set(a, $0) })
    }

    private var isMeasuring: Bool {
        if case .measuring = vm.autoLevelState { true } else { false }
    }

    private var autoLevelText: String {
        switch vm.autoLevelState {
        case .idle: ""
        case .measuring(let s): String(localized: "Measuring… \(s) s", comment: "Auto level countdown, seconds left")
        case .done(let g): String(localized: "Output level set to \(Fmt.signed(g)) dB", comment: "Auto level result")
        case .tooQuiet: String(localized: "Too quiet – play something", comment: "Auto level found no signal")
        }
    }
}

/// "Active on" menu: which output devices AudioCurve processes for, plus a paused notice.
/// A dropdown with one checkbox per output device: AudioCurve processes sound only while a
/// ticked device is the output. A popover (not a menu) so several boxes can be ticked in one go.
private struct ActiveOutputsDropdown: View {
    let vm: PluginViewModel
    @ViewState private var open = false

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            Button { vm.refreshOutputs(); open.toggle() } label: {
                HStack(spacing: 6) {
                    Text(summary).lineLimit(1).truncationMode(.middle)
                    Spacer(minLength: 4)
                    Image(systemName: "chevron.up.chevron.down").imageScale(.small).foregroundStyle(.secondary)
                }
                .frame(maxWidth: .infinity)
            }
            .font(Theme.label)
            .help(Text("Choose the output devices AudioCurve processes sound for. On other outputs the sound passes through unchanged."))
            .popover(isPresented: $open, arrowEdge: .bottom) { checkboxes.padding(12) }
            if !vm.processingActive {
                Text("Paused – \(vm.currentOutput?.name ?? "") is the output", comment: "Status when the current output device is not one the user enabled")
                    .font(Theme.label)
                    .foregroundStyle(Color(nsColor: .systemOrange))
                    .lineLimit(1)
                    .truncationMode(.middle)
            }
        }
    }

    private var checkboxes: some View {
        VStack(alignment: .leading, spacing: 6) {
            let available = vm.availableOutputs
            ForEach(vm.activeOutputs.menuDevices(available: available)) { d in
                let connected = available.contains { $0.uid == d.uid }
                Toggle(isOn: Binding(get: { vm.activeOutputs.isChecked(d.uid) },
                                     set: { vm.setOutput(d, enabled: $0) })) {
                    HStack(spacing: 4) {
                        Text(verbatim: d.name).lineLimit(1)
                        if d.uid == vm.currentOutput?.uid {
                            Text("(current)", comment: "Output device that is in use now").foregroundStyle(.secondary)
                        } else if !connected {
                            Text("(not connected)", comment: "Remembered output device that is not connected").foregroundStyle(.secondary)
                        }
                    }
                }
                .toggleStyle(.checkbox)
                .disabled(vm.activeOutputs.isLastChecked(d.uid, available: available))
            }
        }
        .font(Theme.label)
        .fixedSize()
    }

    private var summary: String {
        let a = vm.activeOutputs
        if a.isAll { return String(localized: "Active on: all outputs", comment: "Active-outputs dropdown label") }
        if a.devices.count == 1 {
            return String(localized: "Active on: \(a.devices[0].name)", comment: "Active-outputs dropdown label with one device")
        }
        return String(localized: "Active on: \(a.devices.count) outputs", comment: "Active-outputs dropdown label with several devices")
    }
}

/// What Auto currently hears. Its own view: it reads the 20 Hz meters.
private struct AutoModeStatus: View {
    let vm: PluginViewModel

    var body: some View {
        let s = vm.meters.speech
        // Same hysteresis idea as the kernel: the label only names a side once it is clear.
        let text = s > 0.7 ? String(localized: "Auto: speech", comment: "Auto mode status")
            : s < 0.3 ? String(localized: "Auto: music", comment: "Auto mode status")
            : String(localized: "Auto: listening…", comment: "Auto mode status: not yet decided")
        Text(text)
            .font(Theme.label)
            .foregroundStyle(.secondary)
            .frame(height: 16, alignment: .leading)
            .accessibilityLabel(text)
    }
}

/// Level and reduction meters. Its own view so the 20 Hz meter refresh only redraws this
/// part of the panel, not the sliders and toggles above it.
private struct MetersBlock: View {
    let vm: PluginViewModel

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            MeterView(title: String(localized: "In", comment: "Short meter label: input"),
                      accessibilityName: String(localized: "Input level"),
                      rms: vm.meters.in_rms_db, peak: vm.meters.in_peak_db)
            MeterView(title: String(localized: "Out", comment: "Short meter label: output"),
                      accessibilityName: String(localized: "Output level"),
                      rms: vm.meters.out_rms_db, peak: vm.meters.out_peak_db)
            HStack {
                Text(verbatim: "\u{2212}60"); Spacer(); Text(verbatim: "\u{2212}30"); Spacer(); Text(verbatim: "0 dBFS")
            }
            .font(.system(size: 11).monospacedDigit())
            .foregroundStyle(.secondary)
            .padding(.leading, MeterView.labelWidth + 8)
            ReductionMeterView(reduction: vm.meters.reduction_db, enabled: vm.limiterOn)
                .padding(.top, 4)
                .help("How much the limiter is lowering the sound right now. If it often shows more than a few dB, lower Output level and turn up the computer volume instead.")
        }
    }
}

private struct SliderRow: View {
    let title: LocalizedStringKey
    @Binding var value: Double
    let range: ClosedRange<Double>
    let step: Double
    let defaultValue: Double
    var minimumLabel: LocalizedStringKey?
    var maximumLabel: LocalizedStringKey?
    let format: (Double) -> String

    /// "Clear to Warm" for sliders with end labels, built from the visible labels.
    private var endLabelsHint: Text {
        guard let minimumLabel, let maximumLabel else { return Text(verbatim: "") }
        return Text(minimumLabel) + Text(verbatim: " – ") + Text(maximumLabel)
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            HStack {
                Text(title).font(Theme.label)
                Spacer()
                Text(format(value))
                    .font(Theme.label.monospacedDigit())
                    .foregroundStyle(.secondary)
            }
            .contentShape(Rectangle())
            .onTapGesture(count: 2) { value = defaultValue }   // double click: back to default
            Slider(value: $value, in: range, step: step)
                .labelsHidden()
                .accessibilityLabel(Text(title))
                .accessibilityValue(format(value))
                .accessibilityHint(endLabelsHint)
            if let minimumLabel, let maximumLabel {
                HStack {
                    Text(minimumLabel); Spacer(); Text(maximumLabel)
                }
                .font(.system(size: 11))
                .foregroundStyle(.secondary)
                .accessibilityHidden(true)
            }
        }
    }
}
