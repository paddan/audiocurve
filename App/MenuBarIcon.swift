import AppKit
import AudioCurveCore

/// The menu bar ear: filled while processing, outline when paused or starting, crossed out when
/// off or failed. Template images, so the menu bar tints them.
enum MenuBarIcon {
    static func image(for state: RouteTarget) -> NSImage {
        switch state {
        case .running: symbol("ear.fill")
        case .paused, .starting: symbol("ear")
        case .off, .failed: crossedOut()
        }
    }

    private static func symbol(_ name: String) -> NSImage {
        let config = NSImage.SymbolConfiguration(pointSize: 15, weight: .regular)
        let img = NSImage(systemSymbolName: name, accessibilityDescription: "AudioCurve")?
            .withSymbolConfiguration(config) ?? NSImage()
        img.isTemplate = true
        return img
    }

    private static func crossedOut() -> NSImage {
        let base = symbol("ear")
        let size = base.size
        let img = NSImage(size: size, flipped: false) { rect in
            base.draw(in: rect)
            // Clear a gap along the slash, then draw it, as SF Symbols' "slash" variants do.
            let path = NSBezierPath()
            path.move(to: NSPoint(x: rect.minX + 1, y: rect.maxY - 1))
            path.line(to: NSPoint(x: rect.maxX - 1, y: rect.minY + 1))
            NSGraphicsContext.current?.compositingOperation = .clear
            path.lineWidth = 3.5
            path.stroke()
            NSGraphicsContext.current?.compositingOperation = .sourceOver
            NSColor.black.setStroke()
            path.lineWidth = 1.5
            path.stroke()
            return true
        }
        img.isTemplate = true
        img.accessibilityDescription = "AudioCurve"
        return img
    }
}
