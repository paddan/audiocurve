import SwiftUI
import CAudioCurveDSP

/// Stereo level meter: RMS bar plus a peak tick per channel, −60…0 dBFS.
struct MeterView: View {
    let title: String
    let rms: (Float, Float)
    let peak: (Float, Float)

    private static let floorDB: Float = -60

    var body: some View {
        HStack(spacing: 8) {
            Text(title)
                .font(Theme.label)
                .frame(width: 24, alignment: .leading)
            Canvas { ctx, size in
                let rowH = (size.height - 2) / 2
                for (i, (r, p)) in [(rms.0, peak.0), (rms.1, peak.1)].enumerated() {
                    let y = CGFloat(i) * (rowH + 2)
                    let track = CGRect(x: 0, y: y, width: size.width, height: rowH)
                    ctx.fill(Path(roundedRect: track, cornerRadius: 2), with: .color(Color.primary.opacity(0.08)))
                    let w = size.width * fraction(r)
                    if w > 0 {
                        ctx.fill(Path(roundedRect: CGRect(x: 0, y: y, width: w, height: rowH), cornerRadius: 2),
                                 with: .color(Color.accentColor.opacity(0.85)))
                    }
                    if p.isFinite, p > Self.floorDB {
                        let px = min(size.width - 1, size.width * fraction(p))
                        ctx.fill(Path(CGRect(x: px - 1, y: y - 1, width: 2, height: rowH + 2)),
                                 with: .color(p > -1 ? Color(nsColor: .systemRed) : Color.primary.opacity(0.8)))
                    }
                }
            }
            .frame(height: 14)
        }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel("\(title)-nivå")
        .accessibilityValue("\(Int(max(rms.0, rms.1).rounded())) dBFS")
    }

    private func fraction(_ db: Float) -> CGFloat {
        guard db.isFinite else { return 0 }
        return CGFloat(min(1, max(0, (db - Self.floorDB) / -Self.floorDB)))
    }
}
