import SwiftUI
import AudioCurveCore
import CAudioCurveDSP

/// Maps frequency (log, 20 Hz...20 kHz) and level (dBFS) to view coordinates.
struct SpectrumGeometry {
    let size: CGSize
    let plot: CGRect

    static let leftInset: CGFloat = 40, rightInset: CGFloat = 20, topInset: CGFloat = 16, bottomInset: CGFloat = 28
    static let fMin = 20.0, fMax = 20_000.0
    static let dbRange: ClosedRange<Double> = -100...0
    static let ticks: [Double] = [50, 100, 200, 500, 1000, 2000, 5000, 10_000, 20_000]

    init(size: CGSize) {
        self.size = size
        plot = CGRect(x: Self.leftInset, y: Self.topInset,
                      width: max(1, size.width - Self.leftInset - Self.rightInset),
                      height: max(1, size.height - Self.topInset - Self.bottomInset))
    }

    func x(_ hz: Double) -> CGFloat {
        plot.minX + CGFloat(log10(hz / Self.fMin) / log10(Self.fMax / Self.fMin)) * plot.width
    }

    func y(_ db: Double) -> CGFloat {
        let c = min(max(db, Self.dbRange.lowerBound), Self.dbRange.upperBound)
        return plot.maxY - CGFloat((c - Self.dbRange.lowerBound) / (Self.dbRange.upperBound - Self.dbRange.lowerBound)) * plot.height
    }
}

/// Plain-array copy of the C spectrum struct (whose arrays import as tuples).
struct SpectrumData {
    var freq: [Double]
    var input: [[Double]]
    var output: [[Double]]

    init(_ s: ac_spectrum) {
        func floats<T>(_ v: T) -> [Double] {
            withUnsafeBytes(of: v) { Array($0.bindMemory(to: Float.self)).map(Double.init) }
        }
        let n = Int(AC_SPECTRUM_POINTS)
        freq = floats(s.freq_hz)
        let i = floats(s.in_db), o = floats(s.out_db)
        input = [Array(i[0..<n]), Array(i[n..<2 * n])]
        output = [Array(o[0..<n]), Array(o[n..<2 * n])]
    }

    /// Level of the point nearest `hz` (on a log axis).
    func level(_ row: [Double], near hz: Double) -> Double {
        guard let i = freq.indices.min(by: { abs(log($0 == 0 ? 1 : freq[$0] / hz)) < abs(log($1 == 0 ? 1 : freq[$1] / hz)) })
        else { return -140 }
        return row[i]
    }

    /// Power average of both ears' input.
    var inputBoth: [Double] {
        zip(input[0], input[1]).map { 10 * log10((pow(10, $0 / 10) + pow(10, $1 / 10)) / 2) }
    }
}

struct SpectrumView: View {
    @Bindable var vm: PluginViewModel

    var body: some View {
        // The picker lives in its own view that never reads `vm.spectrum`: the chart redraws
        // 20 times a second, and rebuilding the segmented control with it made it flicker
        // under the mouse.
        ZStack(alignment: .topLeading) {
            SpectrumChart(vm: vm)
            SpectrumEarPicker(selection: $vm.spectrumEars)
                .padding(.leading, SpectrumGeometry.leftInset + 8)
                .padding(.top, SpectrumGeometry.topInset + 6)
        }
    }
}

private struct SpectrumEarPicker: View {
    @Binding var selection: SpectrumEars

    var body: some View {
        Picker("Öra", selection: $selection) {
            Text("V").tag(SpectrumEars.left)
            Text("H").tag(SpectrumEars.right)
            Text("Båda").tag(SpectrumEars.both)
        }
        .pickerStyle(.segmented)
        .labelsHidden()
        .fixedSize()
    }
}

private struct SpectrumChart: View {
    let vm: PluginViewModel

    private struct Snapshot {
        let data: SpectrumData?
        let ears: SpectrumEars
        let mpo: Double

        var shownEars: [Ear] {
            switch ears {
            case .left: [.left]
            case .right: [.right]
            case .both: [.left, .right]
            }
        }
    }

    var body: some View {
        let snap = Snapshot(data: vm.spectrum.map(SpectrumData.init), ears: vm.spectrumEars, mpo: vm.mpo)
        Canvas { ctx, size in draw(ctx, size, snap) }
            .accessibilityElement()
            .accessibilityLabel("Spektrum")
            .accessibilityValue(describe(snap))
    }

    private func draw(_ ctx: GraphicsContext, _ size: CGSize, _ snap: Snapshot) {
        let g = SpectrumGeometry(size: size)
        let range = SpectrumGeometry.dbRange

        // Plot background and the zone above the ceiling.
        let bg = Path(roundedRect: g.plot, cornerRadius: 6)
        ctx.fill(bg, with: .color(Color.primary.opacity(0.03)))
        let tY = g.y(snap.mpo)
        var over = ctx
        over.clip(to: bg)
        over.fill(Path(CGRect(x: g.plot.minX, y: g.plot.minY, width: g.plot.width, height: max(0, tY - g.plot.minY))),
                  with: .color(Color(nsColor: .systemRed).opacity(0.06)))

        let grid = Color.primary.opacity(0.06)
        for db in stride(from: range.lowerBound, through: range.upperBound, by: 10) {
            var line = Path()
            line.move(to: CGPoint(x: g.plot.minX, y: g.y(db)))
            line.addLine(to: CGPoint(x: g.plot.maxX, y: g.y(db)))
            ctx.stroke(line, with: .color(grid), lineWidth: Int(db) % 20 == 0 ? 1 : 0.5)
            if Int(db) % 20 == 0 {
                ctx.draw(Text(Fmt.db(db)).font(Theme.tick).foregroundStyle(.secondary),
                         at: CGPoint(x: g.plot.minX - 8, y: g.y(db)), anchor: .trailing)
            }
        }
        for f in SpectrumGeometry.ticks {
            var line = Path()
            line.move(to: CGPoint(x: g.x(f), y: g.plot.minY))
            line.addLine(to: CGPoint(x: g.x(f), y: g.plot.maxY))
            ctx.stroke(line, with: .color(grid), lineWidth: 0.5)
            ctx.draw(Text(Fmt.axisFreq(f)).font(Theme.tick).foregroundStyle(.secondary),
                     at: CGPoint(x: g.x(f), y: g.plot.maxY + 8), anchor: .top)
        }

        var plot = ctx
        plot.clip(to: bg)
        if let data = snap.data {
            let input: [Double]
            switch snap.ears {
            case .left: input = data.input[0]
            case .right: input = data.input[1]
            case .both: input = data.inputBoth
            }
            // Input: soft grey area, no hard edge.
            plot.fill(area(data.freq, input, g),
                      with: .linearGradient(Gradient(colors: [Color.secondary.opacity(0.28), Color.secondary.opacity(0.08)]),
                                            startPoint: CGPoint(x: 0, y: g.plot.minY), endPoint: CGPoint(x: 0, y: g.plot.maxY)))
            // Output: ear-coloured line with a faint glow fill under it.
            let fillOpacity = snap.ears == .both ? 0.12 : 0.2
            for ear in snap.shownEars {
                let row = data.output[ear == .left ? 0 : 1]
                let c = Theme.color(ear)
                plot.fill(area(data.freq, row, g),
                          with: .linearGradient(Gradient(colors: [c.opacity(fillOpacity), c.opacity(0)]),
                                                startPoint: CGPoint(x: 0, y: g.plot.minY), endPoint: CGPoint(x: 0, y: g.plot.maxY)))
            }
            for ear in snap.shownEars {
                let row = data.output[ear == .left ? 0 : 1]
                plot.stroke(curve(data.freq, row, g), with: .color(Theme.color(ear)),
                            style: StrokeStyle(lineWidth: 2, lineCap: .round, lineJoin: .round))
            }
        }

        // Output ceiling (MPO).
        var tak = Path()
        tak.move(to: CGPoint(x: g.plot.minX, y: tY))
        tak.addLine(to: CGPoint(x: g.plot.maxX, y: tY))
        ctx.stroke(tak, with: .color(Color(nsColor: .systemRed).opacity(0.7)), style: StrokeStyle(lineWidth: 1, dash: [4, 3]))
        ctx.draw(Text("Tak").font(Theme.tick).foregroundStyle(Color(nsColor: .systemRed).opacity(0.8)),
                 at: CGPoint(x: g.plot.maxX - 6, y: tY + 3), anchor: .topTrailing)
    }

    /// Smooth curve through the points (Catmull-Rom converted to cubic Béziers).
    private func curve(_ freq: [Double], _ db: [Double], _ g: SpectrumGeometry) -> Path {
        let pts = freq.indices.map { CGPoint(x: g.x(freq[$0]), y: g.y(db[$0])) }
        var p = Path()
        guard let first = pts.first else { return p }
        p.move(to: first)
        for i in 1..<pts.count {
            let p0 = pts[max(0, i - 2)], p1 = pts[i - 1], p2 = pts[i], p3 = pts[min(pts.count - 1, i + 1)]
            let c1 = CGPoint(x: p1.x + (p2.x - p0.x) / 6, y: p1.y + (p2.y - p0.y) / 6)
            let c2 = CGPoint(x: p2.x - (p3.x - p1.x) / 6, y: p2.y - (p3.y - p1.y) / 6)
            p.addCurve(to: p2, control1: c1, control2: c2)
        }
        return p
    }

    private func area(_ freq: [Double], _ db: [Double], _ g: SpectrumGeometry) -> Path {
        var a = curve(freq, db, g)
        guard let lastF = freq.last, let firstF = freq.first else { return a }
        a.addLine(to: CGPoint(x: g.x(lastF), y: g.plot.maxY))
        a.addLine(to: CGPoint(x: g.x(firstF), y: g.plot.maxY))
        a.closeSubpath()
        return a
    }

    private func describe(_ snap: Snapshot) -> String {
        guard let data = snap.data else { return "Ingen signal" }
        return snap.shownEars.map { ear in
            let row = data.output[ear == .left ? 0 : 1]
            let parts = [125.0, 1000, 4000].map { "\(Fmt.freq($0)) \(Fmt.db(data.level(row, near: $0))) dBFS" }
            return "\(Fmt.earName(ear)): " + parts.joined(separator: ", ")
        }.joined(separator: ". ")
    }
}
