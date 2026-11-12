import SwiftUI
import AudioCurveCore

enum Theme {
    static let left = Color(red: 0x37 / 255, green: 0x8A / 255, blue: 0xDD / 255)    // #378ADD
    static let right = Color(red: 0xE2 / 255, green: 0x4A / 255, blue: 0x4A / 255)   // #E24B4A
    static func color(_ ear: Ear) -> Color { ear == .left ? left : right }

    static let label = Font.system(size: 13)
    static let tick = Font.system(size: 12).monospacedDigit()
    static let corner: CGFloat = 10
}

struct PluginView: View {
    @Bindable var vm: PluginViewModel

    var body: some View {
        HStack(alignment: .top, spacing: 16) {
            VStack(spacing: 12) {
                header
                chart
                if vm.view == .spectrum { SpectrumLegend() } else { EarLegend(vm: vm) }
            }
            ControlsPanel(vm: vm)
                .frame(width: 232)
        }
        .padding(16)
        .frame(minWidth: 640, minHeight: 380)
        .background(Color(nsColor: .windowBackgroundColor))
        .onAppear { vm.startMeters() }
        .onDisappear { vm.stopMeters() }
    }

    private var header: some View {
        HStack {
            Picker("View", selection: $vm.view) {
                Text("Audiogram").tag(Pane.audiogram)
                Text("Gain").tag(Pane.gain)
                Text("Spectrum").tag(Pane.spectrum)
            }
            .pickerStyle(.segmented)
            .labelsHidden()
            .frame(width: 280)
            if vm.profile != nil { ProfileMenu(vm: vm) }
            Spacer()
            if vm.profile != nil || vm.view == .spectrum {
                Text(vm.view == .audiogram ? LocalizedStringKey("Hearing threshold, dB HL")
                     : vm.view == .gain ? LocalizedStringKey("Gain per band, dB") : LocalizedStringKey("Spectrum, dBFS"))
                    .font(Theme.label)
                    .foregroundStyle(.secondary)
            }
        }
    }

    private var chart: some View {
        ZStack {
            RoundedRectangle(cornerRadius: Theme.corner)
                .fill(Color(nsColor: .textBackgroundColor))
            RoundedRectangle(cornerRadius: Theme.corner)
                .strokeBorder(Color.primary.opacity(0.12), lineWidth: 1)
            if vm.view == .spectrum {
                SpectrumView(vm: vm)
            } else if vm.profile == nil {
                EmptyState(vm: vm)
            } else if vm.view == .audiogram {
                AudiogramEditorView(vm: vm)
            } else {
                GainView(vm: vm)
            }
        }
        .overlay(alignment: .bottom) {
            if let message = vm.errorMessage {
                Text(message)
                    .font(Theme.label.weight(.medium))
                    .foregroundStyle(.white)
                    .padding(.horizontal, 12).padding(.vertical, 6)
                    .background(Capsule().fill(Color(nsColor: .systemRed)))
                    .padding(.bottom, 32)
                    .accessibilityAddTraits(.updatesFrequently)
                    .transition(.opacity)
            }
        }
        .animation(.easeOut(duration: 0.15), value: vm.errorMessage)
    }
}

private struct EmptyState: View {
    let vm: PluginViewModel

    var body: some View {
        VStack(spacing: 12) {
            Text("No audiogram")
                .font(.system(size: 15, weight: .medium))
                .foregroundStyle(.secondary)
            Button("Add audiogram") { vm.addFlatProfile() }
                .buttonStyle(.borderedProminent)
                .controlSize(.large)
        }
    }
}

/// "▒ In   — Ut": key for the spectrum pane.
private struct SpectrumLegend: View {
    var body: some View {
        HStack(spacing: 16) {
            HStack(spacing: 6) {
                RoundedRectangle(cornerRadius: 2).fill(Color.secondary.opacity(0.3)).frame(width: 14, height: 10)
                Text("In", comment: "Short meter label: input")
            }
            HStack(spacing: 6) {
                Capsule().fill(Theme.color(.left)).frame(width: 14, height: 2.5)
                Capsule().fill(Theme.color(.right)).frame(width: 14, height: 2.5)
                Text("Out", comment: "Short meter label: output")
            }
            Spacer()
        }
        .font(Theme.label)
        .foregroundStyle(.secondary)
        .frame(height: 26)
        .accessibilityLabel(Text("Input is the grey area, output is the line"))
    }
}

/// "✕ Vänster  ○ Höger": a click picks the active ear.
private struct EarLegend: View {
    let vm: PluginViewModel

    var body: some View {
        HStack(spacing: 8) {
            ForEach(Ear.allCases, id: \.self) { ear in
                let active = vm.activeEar == ear
                Button {
                    vm.activeEar = ear
                } label: {
                    HStack(spacing: 6) {
                        Text(verbatim: ear == .left ? "✕" : "○")
                            .font(.system(size: 14, weight: .bold))
                            .foregroundStyle(Theme.color(ear))
                        Text(Fmt.earName(ear))
                            .font(Theme.label.weight(active ? .semibold : .regular))
                            .foregroundStyle(active ? Color.primary : Color.secondary)
                    }
                    .padding(.horizontal, 12).padding(.vertical, 4)
                    .background(Capsule().fill(Theme.color(ear).opacity(active ? 0.16 : 0)))
                    .overlay(Capsule().strokeBorder(Theme.color(ear).opacity(active ? 0.5 : 0), lineWidth: 1))
                    .contentShape(Capsule())
                }
                .buttonStyle(.plain)
                .fixedSize()
                .accessibilityLabel(ear == .left ? Text("Left ear") : Text("Right ear"))
                .accessibilityAddTraits(active ? .isSelected : [])
            }
            Spacer()
            if vm.profile != nil && vm.view == .gain {
                Text("Bar: soft · dashed: loud")
                    .font(Theme.label)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
                    .layoutPriority(-1)
            }
        }
        .frame(height: 26)
    }
}

// MARK: - Shared canvas helpers

extension GraphicsContext {
    /// Draws a small rounded label with an ear-coloured outline, kept inside `bounds`.
    func drawPill(_ text: String, near anchor: CGPoint, gap: CGFloat, in bounds: CGRect, color: Color,
                  background: Color) {
        let resolved = resolve(Text(text).font(.system(size: 13).monospacedDigit()).foregroundStyle(Color.primary))
        let size = resolved.measure(in: CGSize(width: 400, height: 40))
        let w = size.width + 14, h = size.height + 6
        var y = anchor.y - gap - h
        if y < bounds.minY { y = anchor.y + gap }
        let x = min(max(anchor.x - w / 2, bounds.minX), bounds.maxX - w)
        let rect = CGRect(x: x, y: y, width: w, height: h)
        let path = Path(roundedRect: rect, cornerRadius: h / 2)
        fill(path, with: .color(background))
        stroke(path, with: .color(color), lineWidth: 1)
        draw(resolved, at: CGPoint(x: rect.midX, y: rect.midY), anchor: .center)
    }
}

func optionKeyDown() -> Bool { NSEvent.modifierFlags.contains(.option) }
