import SwiftUI
import AudioCurveCore

struct GainGeometry {
    let size: CGSize
    let plot: CGRect
    let range: ClosedRange<Double>

    static let leftInset: CGFloat = 40, rightInset: CGFloat = 20, topInset: CGFloat = 16, bottomInset: CGFloat = 28

    init(size: CGSize, range: ClosedRange<Double>) {
        self.size = size
        self.range = range
        plot = CGRect(x: Self.leftInset, y: Self.topInset,
                      width: max(1, size.width - Self.leftInset - Self.rightInset),
                      height: max(1, size.height - Self.topInset - Self.bottomInset))
    }

    var slotWidth: CGFloat { plot.width / CGFloat(Bands.count) }
    var barWidth: CGFloat { min(18, slotWidth * 0.36) }

    func y(_ db: Double) -> CGFloat {
        plot.maxY - CGFloat((db - range.lowerBound) / (range.upperBound - range.lowerBound)) * plot.height
    }

    func barRect(ear: Ear, band: Int, value: Double) -> CGRect {
        let center = plot.minX + slotWidth * (CGFloat(band) + 0.5)
        let x = ear == .left ? center - barWidth - 1 : center + 1
        let y0 = y(0), y1 = y(value)
        return CGRect(x: x, y: min(y0, y1), width: barWidth, height: max(1, abs(y1 - y0)))
    }

    /// Which bar column (band + ear) a point falls on, if inside the plot.
    func hit(_ p: CGPoint) -> TrimSelection? {
        guard p.x >= plot.minX, p.x <= plot.maxX, p.y >= plot.minY - 12, p.y <= plot.maxY + 12 else { return nil }
        let band = min(Bands.count - 1, max(0, Int((p.x - plot.minX) / slotWidth)))
        let center = plot.minX + slotWidth * (CGFloat(band) + 0.5)
        return TrimSelection(ear: p.x < center ? .left : .right, band: band)
    }
}

struct GainView: View {
    let vm: PluginViewModel
    @FocusState private var focused: Bool
    @ViewState private var dragging = false

    /// Everything the canvas draws, read in `body` so Observation tracks it and triggers redraws.
    private struct Snapshot {
        let profile: HearingProfile?
        let range: ClosedRange<Double>
        let fits: [Ear: [BandFitting]]
        let both: [Ear: (speech: [BandFitting], music: [BandFitting])]
        let trimSelection: TrimSelection?
        let label: String
    }

    var body: some View {
        let snap = Snapshot(profile: vm.profile, range: vm.gainRange,
                            fits: [.left: vm.fitting(.left), .right: vm.fitting(.right)],
                            both: [.left: vm.fittings(.left), .right: vm.fittings(.right)],
                            trimSelection: vm.trimSelection,
                            label: vm.trimSelection.map { vm.label(for: $0) } ?? "")
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
                            vm.drag(to: v.location, in: geo.size, fine: false)
                        }
                        .onEnded { _ in dragging = false; vm.endDrag() }
                )
                .simultaneousGesture(
                    SpatialTapGesture(count: 2).onEnded { vm.resetTrim(at: $0.location, in: geo.size) }
                )
                .accessibilityLabel(Text("Gain per band"))
                .accessibilityChildren { barElements(geo.size) }
        }
        .focusable()
        .focused($focused)
        .focusEffectDisabled()
        .onKeyPress(phases: [.down, .repeat]) { vm.handleKey($0) }
    }

    private func draw(_ ctx: GraphicsContext, _ size: CGSize, _ snap: Snapshot) {
        guard let profile = snap.profile else { return }
        let range = snap.range
        let g = GainGeometry(size: size, range: range)
        let grid = Color.primary.opacity(0.08)

        for db in stride(from: range.lowerBound, through: range.upperBound, by: 10) {
            var line = Path()
            line.move(to: CGPoint(x: g.plot.minX, y: g.y(db)))
            line.addLine(to: CGPoint(x: g.plot.maxX, y: g.y(db)))
            ctx.stroke(line, with: .color(db == 0 ? Color.primary.opacity(0.3) : grid), lineWidth: 1)
            ctx.draw(Text(Fmt.db(db)).font(Theme.tick).foregroundStyle(.secondary),
                     at: CGPoint(x: g.plot.minX - 8, y: g.y(db)), anchor: .trailing)
        }

        for (band, f) in Bands.centers.enumerated() {
            ctx.draw(Text(Fmt.axisFreq(f)).font(Theme.tick).foregroundStyle(.secondary),
                     at: CGPoint(x: g.plot.minX + g.slotWidth * (CGFloat(band) + 0.5), y: g.plot.maxY + 8),
                     anchor: .top)
        }

        for ear in Ear.allCases {
            let color = Theme.color(ear)
            let fits = snap.fits[ear] ?? []
            let trims = profile.trim(ear)
            for band in 0..<Bands.count {
                let soft = fits[band].gainSoftDB
                let rect = g.barRect(ear: ear, band: band, value: soft)
                let trimmed = trims[band] != 0
                ctx.fill(Path(roundedRect: rect, cornerRadius: 2), with: .color(color.opacity(trimmed ? 0.85 : 0.6)))
                // Gain for loud input: dashed marker across the bar.
                var dash = Path()
                let yl = g.y(fits[band].gainLoudDB)
                dash.move(to: CGPoint(x: rect.minX - 2, y: yl))
                dash.addLine(to: CGPoint(x: rect.maxX + 2, y: yl))
                ctx.stroke(dash, with: .color(Color.primary.opacity(0.85)),
                           style: StrokeStyle(lineWidth: 1.5, dash: [3, 2]))
                // Soft gain of each prescription: speech solid, music dashed. Drawing only, no hit-testing.
                if let two = snap.both[ear], two.speech.indices.contains(band), two.music.indices.contains(band) {
                    for (gain, dashed) in [(two.speech[band].gainSoftDB, false), (two.music[band].gainSoftDB, true)] {
                        var tick = Path()
                        let yt = g.y(gain)
                        tick.move(to: CGPoint(x: rect.minX - 3, y: yt))
                        tick.addLine(to: CGPoint(x: rect.maxX + 3, y: yt))
                        ctx.stroke(tick, with: .color(Color.secondary),
                                   style: StrokeStyle(lineWidth: 1, dash: dashed ? [2, 2] : []))
                    }
                }
            }
        }

        if let t = snap.trimSelection, let tf = snap.fits[t.ear], tf.indices.contains(t.band) {
            let soft = tf[t.band].gainSoftDB
            let rect = g.barRect(ear: t.ear, band: t.band, value: soft)
            ctx.stroke(Path(roundedRect: rect.insetBy(dx: -3, dy: -3), cornerRadius: 4),
                       with: .color(Color.accentColor), lineWidth: 2)
            ctx.drawPill(snap.label, near: CGPoint(x: rect.midX, y: rect.minY), gap: 10, in: g.plot,
                         color: Theme.color(t.ear), background: Color(nsColor: .windowBackgroundColor))
        }
    }

    private struct BarItem: Identifiable {
        let sel: TrimSelection
        let trim: Double
        var id: String { "\(sel.ear.rawValue)-\(sel.band)" }
    }

    private var items: [BarItem] {
        guard let p = vm.profile else { return [] }
        return Ear.allCases.flatMap { ear in
            (0..<Bands.count).map { BarItem(sel: TrimSelection(ear: ear, band: $0), trim: p.trim(ear)[$0]) }
        }
    }

    @ViewBuilder private func barElements(_ size: CGSize) -> some View {
        let g = GainGeometry(size: size, range: vm.gainRange)
        ForEach(items) { item in
            let r = g.barRect(ear: item.sel.ear, band: item.sel.band, value: 0)
            Rectangle().fill(Color.clear)
                .frame(width: g.barWidth, height: g.plot.height)
                .position(x: r.midX, y: g.plot.midY)
                .accessibilityElement()
                .accessibilityLabel(Text("\(Fmt.earName(item.sel.ear)) \(Fmt.freq(Bands.centers[item.sel.band])) trim", comment: "VoiceOver: ear, band frequency, trim"))
                .accessibilityValue(Text(verbatim: "\(Fmt.signed(item.trim)) dB"))
                .accessibilityAdjustableAction { direction in
                    switch direction {
                    case .increment: vm.accessibilityAdjustTrim(item.sel, up: true)
                    case .decrement: vm.accessibilityAdjustTrim(item.sel, up: false)
                    @unknown default: break
                    }
                }
        }
    }
}
