import Foundation

/// A single audiogram point, addressed by ear and frequency.
public struct PointSelection: Equatable, Sendable {
    public var ear: Ear
    public var frequencyHz: Double

    public init(ear: Ear, frequencyHz: Double) {
        self.ear = ear
        self.frequencyHz = frequencyHz
    }
}

/// Pure editing operations on a `HearingProfile` for click, drag and keyboard editing of the audiogram.
public enum AudiogramEditing {
    public static let minPoints = 4

    /// Rounds to the nearest 5 dB (or 1 dB when fine), then clamps to −10…120.
    public static func snap(_ dbHL: Double, fine: Bool) -> Double {
        let step = fine ? 1.0 : 5.0
        let rounded = (dbHL / step).rounded() * step
        return min(120, max(-10, rounded))
    }

    /// Sets the threshold at the selected frequency. Adds the point if missing, but only for standard frequencies.
    public static func setValue(_ p: inout HearingProfile, _ s: PointSelection, dbHL: Double, fine: Bool) {
        let value = snap(dbHL, fine: fine)
        var a = p.audiogram(s.ear)
        if let i = a.points.firstIndex(where: { $0.frequencyHz == s.frequencyHz }) {
            a.points[i].thresholdDBHL = value
        } else if Audiogram.standardFrequencies.contains(s.frequencyHz) {
            a.points.append(AudiogramPoint(frequencyHz: s.frequencyHz, thresholdDBHL: value))
            a.points.sort { $0.frequencyHz < $1.frequencyHz }
        } else {
            return
        }
        p.setAudiogram(a, for: s.ear)
    }

    /// Moves the threshold one step: `up` lowers dB HL (−5, or −1 when fine), otherwise raises it. No-op if the point is missing.
    public static func nudge(_ p: inout HearingProfile, _ s: PointSelection, up: Bool, fine: Bool) {
        var a = p.audiogram(s.ear)
        guard let i = a.points.firstIndex(where: { $0.frequencyHz == s.frequencyHz }) else { return }
        let step = fine ? 1.0 : 5.0
        let delta = up ? -step : step
        a.points[i].thresholdDBHL = snap(a.points[i].thresholdDBHL + delta, fine: fine)
        p.setAudiogram(a, for: s.ear)
    }

    /// The next (or previous) existing point on the same ear, skipping missing frequencies.
    public static func neighbor(_ p: HearingProfile, of s: PointSelection, forward: Bool) -> PointSelection? {
        let points = p.audiogram(s.ear).points
        let target: AudiogramPoint?
        if forward {
            target = points.first { $0.frequencyHz > s.frequencyHz }
        } else {
            target = points.last { $0.frequencyHz < s.frequencyHz }
        }
        return target.map { PointSelection(ear: s.ear, frequencyHz: $0.frequencyHz) }
    }

    /// Same frequency on the other ear if present, otherwise the nearest one in log-frequency (ties go to the higher).
    /// If the other ear has no points, the selection is returned unchanged.
    public static func switchEar(_ p: HearingProfile, _ s: PointSelection) -> PointSelection {
        let other: Ear = s.ear == .left ? .right : .left
        let points = p.audiogram(other).points
        if points.isEmpty { return s }
        if points.contains(where: { $0.frequencyHz == s.frequencyHz }) {
            return PointSelection(ear: other, frequencyHz: s.frequencyHz)
        }
        let target = log2(s.frequencyHz)
        var best = points[0]
        var bestDistance = abs(log2(best.frequencyHz) - target)
        for candidate in points.dropFirst() {
            let distance = abs(log2(candidate.frequencyHz) - target)
            if distance < bestDistance - 1e-12
                || (abs(distance - bestDistance) <= 1e-12 && candidate.frequencyHz > best.frequencyHz) {
                best = candidate
                bestDistance = distance
            }
        }
        return PointSelection(ear: other, frequencyHz: best.frequencyHz)
    }

    /// Removes the selected point. Returns false (and leaves the profile unchanged) if the point is missing
    /// or if fewer than `minPoints` would remain.
    public static func delete(_ p: inout HearingProfile, _ s: PointSelection) -> Bool {
        var a = p.audiogram(s.ear)
        guard a.points.count > minPoints,
              let i = a.points.firstIndex(where: { $0.frequencyHz == s.frequencyHz }) else { return false }
        a.points.remove(at: i)
        p.setAudiogram(a, for: s.ear)
        return true
    }

    /// Parses a typed dB HL value. Accepts ASCII "-" and Unicode minus "−"; returns nil outside −10…120.
    public static func parseTyped(_ text: String) -> Double? {
        let normalized = text
            .trimmingCharacters(in: .whitespacesAndNewlines)
            .replacingOccurrences(of: "\u{2212}", with: "-")
        guard let value = Double(normalized), value >= -10, value <= 120 else { return nil }
        return value
    }

    /// Maps a vertical position in the plot to dB HL: y = 0 is −10, y = plotHeight is 120. Not rounded.
    public static func dbHL(atY y: Double, plotHeight: Double) -> Double {
        guard plotHeight > 0 else { return -10 }
        return -10 + y / plotHeight * 130
    }

    /// Adds `delta` to the band's trim, clamped to ±20 dB.
    public static func nudgeTrim(_ p: inout HearingProfile, ear: Ear, band: Int, delta: Double) {
        var t = p.trim(ear)
        guard t.indices.contains(band) else { return }
        t[band] = min(20, max(-20, t[band] + delta))
        p.setTrim(t, for: ear)
    }

    /// Sets the band's trim back to 0 dB.
    public static func resetTrim(_ p: inout HearingProfile, ear: Ear, band: Int) {
        var t = p.trim(ear)
        guard t.indices.contains(band) else { return }
        t[band] = 0
        p.setTrim(t, for: ear)
    }
}
