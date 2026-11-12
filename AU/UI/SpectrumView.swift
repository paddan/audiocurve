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

    private struct Snapshot {
        let data: SpectrumData?
        let ears: SpectrumEars
        let mpo: Double
    }

    private var shownEars: [Ear] {
        switch vm.spectrumEars {
        case .left: [.left]
        case .right: [.right]
        case .both: [.left, .right]
        }
    }

    var body: some View {
        let snap = Snapshot(data: vm.spectrum.map(SpectrumData.init), ears: vm.spectrumEars, mpo: vm.mpo)
        ZStack(alignment: .topLeading) {
            Canvas { ctx, size in draw(ctx, size, snap) }
                .accessibilityElement()
                .accessibilityLabel("Spektrum")
                .accessibilityValue(describe(snap))
            Picker("Öra", selection: $vm.spectrumEars) {
                Text("V").tag(SpectrumEars.left)
                Text("H").tag(SpectrumEars.right)
                Text("Båda").tag(SpectrumEars.both)
            }
            .pickerStyle(.segmented)
            .labelsHidden()
            .frame(width: 130)
            .padding(.leading, SpectrumGeometry.leftInset + 8)
            .padding(.top, SpectrumGeometry.topInset + 6)
        }
    }

    private func draw(_ ctx: GraphicsContext, _ size: CGSize, _ snap: Snapshot) {
        let g = SpectrumGeometry(size: size)
        let grid = Color.primary.opacity(0.08)
        let range = SpectrumGeometry.dbRange

        for db in stride(from: range.lowerBound, through: range.upperBound, by: 10) {
            var line = Path()
            line.move(to: CGPoint(x: g.plot.minX, y: g.y(db)))
            line.addLine(to: CGPoint(x: g.plot.maxX, y: g.y(db)))
            ctx.stroke(line, with: .color(db == 0 ? Color.primary.opacity(0.3) : grid), lineWidth: 1)
            if Int(db) % 20 == 0 {
                ctx.draw(Text(Fmt.db(db)).font(Theme.tick).foregroundStyle(.secondary),
                         at: CGPoint(x: g.plot.minX - 8, y: g.y(db)), anchor: .trailing)
            }
        }
        for f in SpectrumGeometry.ticks {
            var line = Path()
            line.move(to: CGPoint(x: g.x(f), y: g.plot.minY))
            line.addLine(to: CGPoint(x: g.x(f), y: g.plot.maxY))
            ctx.stroke(line, with: .color(grid), lineWidth: 1)
            ctx.draw(Text(Fmt.axisFreq(f)).font(Theme.tick).foregroundStyle(.secondary),
                     at: CGPoint(x: g.x(f), y: g.plot.maxY + 8), anchor: .top)
        }

        if let data = snap.data {
            let inputs: [[Double]]
            switch snap.ears {
            case .left: inputs = [data.input[0]]
            case .right: inputs = [data.input[1]]
            case .both: inputs = [data.inputBoth]
            }
            for row in inputs {
                var area = curve(data.freq, row, g)
                let outline = area
                area.addLine(to: CGPoint(x: g.x(data.freq[data.freq.count - 1]), y: g.plot.maxY))
                area.addLine(to: CGPoint(x: g.x(data.freq[0]), y: g.plot.maxY))
                area.closeSubpath()
                ctx.fill(area, with: .color(Color.secondary.opacity(0.18)))
                ctx.stroke(outline, with: .color(Color.secondary.opacity(0.5)), lineWidth: 1)
            }
            for ear in shownEars {
                let row = data.output[ear == .left ? 0 : 1]
                ctx.stroke(curve(data.freq, row, g), with: .color(Theme.color(ear)),
                           style: StrokeStyle(lineWidth: 1.5, lineJoin: .round))
            }
        }

        // Output ceiling (MPO).
        var tak = Path()
        tak.move(to: CGPoint(x: g.plot.minX, y: g.y(snap.mpo)))
        tak.addLine(to: CGPoint(x: g.plot.maxX, y: g.y(snap.mpo)))
        ctx.stroke(tak, with: .color(Color.primary.opacity(0.6)), style: StrokeStyle(lineWidth: 1, dash: [4, 3]))
        ctx.draw(Text("Tak").font(Theme.tick).foregroundStyle(.secondary),
                 at: CGPoint(x: g.plot.maxX - 4, y: g.y(snap.mpo) + 3), anchor: .topTrailing)
    }

    private func curve(_ freq: [Double], _ db: [Double], _ g: SpectrumGeometry) -> Path {
        var p = Path()
        for (i, f) in freq.enumerated() {
            let pt = CGPoint(x: g.x(f), y: g.y(db[i]))
            i == 0 ? p.move(to: pt) : p.addLine(to: pt)
        }
        return p
    }

    private func describe(_ snap: Snapshot) -> String {
        guard let data = snap.data else { return "Ingen signal" }
        return shownEars.map { ear in
            let row = data.output[ear == .left ? 0 : 1]
            let parts = [125.0, 1000, 4000].map { "\(Fmt.freq($0)) \(Fmt.db(data.level(row, near: $0))) dBFS" }
            return "\(Fmt.earName(ear)): " + parts.joined(separator: ", ")
        }.joined(separator: ". ")
    }
}
