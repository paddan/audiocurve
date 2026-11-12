import SwiftUI
import AudioCurveCore

/// Maps audiogram data to view coordinates. Shared by drawing and hit testing.
struct PlotGeometry {
    let size: CGSize
    let plot: CGRect

    static let leftInset: CGFloat = 40, rightInset: CGFloat = 20, topInset: CGFloat = 16, bottomInset: CGFloat = 28
    static let octaves = 6.0       // 125 ... 8000 Hz

    init(size: CGSize) {
        self.size = size
        plot = CGRect(x: Self.leftInset, y: Self.topInset,
                      width: max(1, size.width - Self.leftInset - Self.rightInset),
                      height: max(1, size.height - Self.topInset - Self.bottomInset))
    }

    func x(_ hz: Double) -> CGFloat { plot.minX + CGFloat(log2(hz / 125) / Self.octaves) * plot.width }
    func y(_ dbHL: Double) -> CGFloat { plot.minY + CGFloat((dbHL + 10) / 130) * plot.height }
}

struct AudiogramEditorView: View {
    let vm: PluginViewModel
    @FocusState private var focused: Bool
    @State private var dragging = false

    /// Everything the canvas draws, read in `body` so Observation tracks it and triggers redraws.
    private struct Snapshot {
        let profile: HearingProfile?
        let selection: PointSelection?
        let activeEar: Ear
        let label: String
    }

    var body: some View {
        let snap = Snapshot(profile: vm.profile, selection: vm.selection, activeEar: vm.activeEar,
                            label: vm.selection.map { vm.label(for: $0) } ?? "")
        GeometryReader { geo in
            Canvas { ctx, size in draw(ctx, size, snap) }
                .contentShape(Rectangle())
                .gesture(
                    DragGesture(minimumDistance: 0)
                        .onChanged { v in
                            if !dragging {
                                dragging = true
                                focused = true
                                vm.beginDrag(at: v.startLocation, in: geo.size)
                            }
                            vm.drag(to: v.location, in: geo.size, fine: optionKeyDown())
                        }
                        .onEnded { _ in dragging = false; vm.endDrag() }
                )
                .accessibilityLabel(Text("Audiogram"))
                .accessibilityChildren { pointElements(geo.size) }
        }
        .focusable()
        .focused($focused)
        .focusEffectDisabled()
        .onKeyPress(phases: [.down, .repeat]) { vm.handleKey($0) }
    }

    // MARK: Drawing

    private func draw(_ ctx: GraphicsContext, _ size: CGSize, _ snap: Snapshot) {
        guard let profile = snap.profile else { return }
        let g = PlotGeometry(size: size)
        let grid = Color.primary.opacity(0.08)

        // Normal-hearing band (−10…20 dB HL).
        ctx.fill(Path(CGRect(x: g.plot.minX, y: g.y(-10), width: g.plot.width, height: g.y(20) - g.y(-10))),
                 with: .color(Color.green.opacity(0.10)))

        // Horizontal grid every 10 dB, labels every 20.
        for db in stride(from: -10.0, through: 120.0, by: 10.0) {
            var line = Path()
            line.move(to: CGPoint(x: g.plot.minX, y: g.y(db)))
            line.addLine(to: CGPoint(x: g.plot.maxX, y: g.y(db)))
            ctx.stroke(line, with: .color(db == 0 ? Color.primary.opacity(0.22) : grid), lineWidth: 1)
            if db >= 0 && Int(db) % 20 == 0 {
                ctx.draw(Text(Fmt.db(db)).font(Theme.tick).foregroundStyle(.secondary),
                         at: CGPoint(x: g.plot.minX - 8, y: g.y(db)), anchor: .trailing)
            }
        }

        // Vertical grid at the standard frequencies.
        for f in Audiogram.standardFrequencies {
            var line = Path()
            line.move(to: CGPoint(x: g.x(f), y: g.plot.minY))
            line.addLine(to: CGPoint(x: g.x(f), y: g.plot.maxY))
            ctx.stroke(line, with: .color(grid), lineWidth: 1)
            ctx.draw(Text(Fmt.axisFreq(f)).font(Theme.tick).foregroundStyle(.secondary),
                     at: CGPoint(x: g.x(f), y: g.plot.maxY + 8), anchor: .top)
        }

        // Curves and markers: inactive ear first so the active one is on top.
        let ears: [Ear] = snap.activeEar == .left ? [.right, .left] : [.left, .right]
        for ear in ears {
            let active = ear == snap.activeEar
            let color = Theme.color(ear)
            let pts = profile.audiogram(ear).points.map { CGPoint(x: g.x($0.frequencyHz), y: g.y($0.thresholdDBHL)) }
            var path = Path()
            for (i, p) in pts.enumerated() { i == 0 ? path.move(to: p) : path.addLine(to: p) }
            ctx.stroke(path, with: .color(color.opacity(active ? 0.7 : 0.4)),
                       style: StrokeStyle(lineWidth: 1.5, lineJoin: .round))
            for p in pts { drawMarker(ctx, ear: ear, at: p, color: color.opacity(active ? 1 : 0.75)) }
        }

        // Selected point: focus ring and label.
        if let s = snap.selection,
           let pt = profile.audiogram(s.ear).points.first(where: { $0.frequencyHz == s.frequencyHz }) {
            let c = CGPoint(x: g.x(pt.frequencyHz), y: g.y(pt.thresholdDBHL))
            ctx.stroke(Path(ellipseIn: CGRect(x: c.x - 11, y: c.y - 11, width: 22, height: 22)),
                       with: .color(Color.accentColor), lineWidth: 2)
            ctx.drawPill(snap.label, near: c, gap: 16, in: g.plot, color: Theme.color(s.ear),
                         background: Color(nsColor: .windowBackgroundColor))
        }
    }

    private func drawMarker(_ ctx: GraphicsContext, ear: Ear, at p: CGPoint, color: Color) {
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

    // MARK: VoiceOver

    private struct PointItem: Identifiable {
        let sel: PointSelection
        let dbHL: Double
        var id: String { "\(sel.ear.rawValue)-\(sel.frequencyHz)" }
    }

    private var items: [PointItem] {
        guard let p = vm.profile else { return [] }
        return Ear.allCases.flatMap { ear in
            p.audiogram(ear).points.map { PointItem(sel: PointSelection(ear: ear, frequencyHz: $0.frequencyHz), dbHL: $0.thresholdDBHL) }
        }
    }

    @ViewBuilder private func pointElements(_ size: CGSize) -> some View {
        let g = PlotGeometry(size: size)
        ForEach(items) { item in
            Rectangle().fill(Color.clear)
                .frame(width: 24, height: 24)
                .position(x: g.x(item.sel.frequencyHz), y: g.y(item.dbHL))
                .accessibilityElement()
                .accessibilityLabel(Text(verbatim: "\(Fmt.earName(item.sel.ear)) \(Fmt.freq(item.sel.frequencyHz))"))
                .accessibilityValue(Text(verbatim: "\(Fmt.db(item.dbHL)) dB HL"))
                .accessibilityAdjustableAction { direction in
                    switch direction {
                    case .increment: vm.accessibilityAdjust(item.sel, up: true)     // up = −5 dB HL
                    case .decrement: vm.accessibilityAdjust(item.sel, up: false)
                    @unknown default: break
                    }
                }
        }
    }
}
