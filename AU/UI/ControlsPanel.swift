import SwiftUI
import AudioToolbox
import AudioCurveCore

struct ControlsPanel: View {
    @Bindable var vm: PluginViewModel

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Picker("Läge", selection: Binding(get: { vm.mode }, set: { vm.setMode($0) })) {
                Text("Tal").tag(CompressionMode.speech)
                Text("Musik").tag(CompressionMode.music)
            }
            .pickerStyle(.segmented)
            .labelsHidden()

            SliderRow(title: "Styrka", value: binding(.strength), range: 0...100, step: 1, defaultValue: 50) {
                "\(Int($0.rounded())) %"
            }
            SliderRow(title: "Utnivå", value: binding(.outputGain), range: -48...12, step: 0.5, defaultValue: 0) {
                "\(Fmt.signed($0)) dB"
            }
            SliderRow(title: "Tak (MPO)", value: binding(.mpo), range: -20 ... -1, step: 1, defaultValue: -3) {
                "\(Fmt.db($0)) dBFS"
            }

            HStack(spacing: 16) {
                Toggle("Kompression", isOn: Binding(get: { vm.compressionOn }, set: { vm.set(.compression, $0 ? 1 : 0) }))
                Toggle("Limiter", isOn: Binding(get: { vm.limiterOn }, set: { vm.set(.limiter, $0 ? 1 : 0) }))
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

            VStack(alignment: .leading, spacing: 6) {
                MeterView(title: "In", rms: vm.meters.in_rms_db, peak: vm.meters.in_peak_db)
                MeterView(title: "Ut", rms: vm.meters.out_rms_db, peak: vm.meters.out_peak_db)
                HStack {
                    Text("\u{2212}60"); Spacer(); Text("\u{2212}30"); Spacer(); Text("0 dBFS")
                }
                .font(.system(size: 11).monospacedDigit())
                .foregroundStyle(.secondary)
                .padding(.leading, 32)
            }
        }
        .padding(16)
        .frame(maxHeight: .infinity, alignment: .top)
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
