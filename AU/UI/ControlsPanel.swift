import SwiftUI
import AudioToolbox
import AudioCurveCore

struct ControlsPanel: View {
    @Bindable var vm: PluginViewModel
    @State private var confirmLimiterOff = false

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Picker("Läge", selection: Binding(get: { vm.mode }, set: { vm.setMode($0) })) {
                Text("Tal").tag(CompressionMode.speech)
                Text("Musik").tag(CompressionMode.music)
                Text("Auto").tag(CompressionMode.auto)
            }
            .pickerStyle(.segmented)
            .labelsHidden()
            if vm.mode == .auto {
                AutoModeStatus(vm: vm)
            }

            SliderRow(title: "Styrka", value: binding(.strength), range: 0...100, step: 1, defaultValue: 50) {
                "\(Int($0.rounded())) %"
            }
            SliderRow(title: "Innivå", value: binding(.inputGain), range: -30...12, step: 0.5, defaultValue: 0) {
                "\(Fmt.signed($0)) dB"
            }
            .help("Sänker eller höjer ljudet innan det förstärks. Lägre innivå gör att kompressionen arbetar mindre.")
            SliderRow(title: "Utnivå", value: binding(.outputGain), range: -48...12, step: 0.5, defaultValue: 0) {
                "\(Fmt.signed($0)) dB"
            }
            SliderRow(title: "Tak (MPO)", value: binding(.mpo), range: -20 ... -1, step: 1, defaultValue: -3) {
                "\(Fmt.db($0)) dBFS"
            }

            HStack(spacing: 16) {
                Toggle("Kompression", isOn: Binding(get: { vm.compressionOn }, set: { vm.set(.compression, $0 ? 1 : 0) }))
                Toggle("Limiter", isOn: Binding(get: { vm.limiterOn }, set: { on in
                    // Turning the safety stage off needs a confirmation; turning it on never does.
                    if on { vm.set(.limiter, 1) } else { confirmLimiterOff = true }
                }))
            }
            .toggleStyle(.checkbox)
            .font(Theme.label)
            if !vm.limiterOn {
                Text("Limiter av: ingen nivåspärr under 0 dBFS")
                    .font(Theme.label)
                    .foregroundStyle(Color(nsColor: .systemOrange))
            }

            VStack(alignment: .leading, spacing: 4) {
                Button { vm.startAutoLevel() } label: {
                    Text("Auto-nivå").frame(maxWidth: .infinity)
                }
                .controlSize(.large)
                .disabled(isMeasuring)
                Text(autoLevelText)
                    .font(Theme.label)
                    .foregroundStyle(vm.autoLevelState == .tooQuiet ? Color(nsColor: .systemOrange) : .secondary)
                    .frame(height: 16, alignment: .leading)
            }

            Spacer(minLength: 0)

            MetersBlock(vm: vm)
        }
        .padding(16)
        .frame(maxHeight: .infinity, alignment: .top)
        .alert("Stänga av limitern?", isPresented: $confirmLimiterOff) {
            Button("Stäng av", role: .destructive) { vm.set(.limiter, 0) }
            Button("Avbryt", role: .cancel) {}
        } message: {
            Text("Utan limiter finns ingen nivåspärr under 0 dBFS. Med stark förstärkning kan ljudet bli mycket högt och skada hörseln. Sänk volymen innan du stänger av.")
        }
        .background(RoundedRectangle(cornerRadius: Theme.corner).fill(Color.primary.opacity(0.04)))
        .overlay(RoundedRectangle(cornerRadius: Theme.corner).strokeBorder(Color.primary.opacity(0.08), lineWidth: 1))
    }

    private func binding(_ a: ParamAddress) -> Binding<Double> {
        Binding(
            get: {
                switch a {
                case .strength: vm.strength
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
        case .measuring(let s): "Mäter… \(s) s"
        case .done(let g): "Utnivå satt till \(Fmt.signed(g)) dB"
        case .tooQuiet: "För tyst – spela upp något"
        }
    }
}

/// What Auto currently hears. Its own view: it reads the 20 Hz meters.
private struct AutoModeStatus: View {
    let vm: PluginViewModel

    var body: some View {
        let s = vm.meters.speech
        // Same hysteresis idea as the kernel: the label only names a side once it is clear.
        let text = s > 0.7 ? "Auto: tal" : s < 0.3 ? "Auto: musik" : "Auto: lyssnar…"
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
            MeterView(title: "In", rms: vm.meters.in_rms_db, peak: vm.meters.in_peak_db)
            MeterView(title: "Ut", rms: vm.meters.out_rms_db, peak: vm.meters.out_peak_db)
            HStack {
                Text("\u{2212}60"); Spacer(); Text("\u{2212}30"); Spacer(); Text("0 dBFS")
            }
            .font(.system(size: 11).monospacedDigit())
            .foregroundStyle(.secondary)
            .padding(.leading, 32)
            ReductionMeterView(reduction: vm.meters.reduction_db, enabled: vm.limiterOn)
                .padding(.top, 4)
                .help("Hur mycket limitern sänker ljudet just nu. Ligger den ofta över några dB: sänk Utnivå och höj volymen på datorn i stället.")
        }
    }
}

private struct SliderRow: View {
    let title: String
    @Binding var value: Double
    let range: ClosedRange<Double>
    let step: Double
    let defaultValue: Double
    let format: (Double) -> String

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
                .accessibilityLabel(title)
                .accessibilityValue(format(value))
        }
    }
}
