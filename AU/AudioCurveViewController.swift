import AppKit
import AudioToolbox
import CoreAudioKit

public final class AudioCurveViewController: AUViewController, AUAudioUnitFactory {
    public override func loadView() {
        let label = NSTextField(labelWithString: "audiocurve")
        label.font = .systemFont(ofSize: 24, weight: .semibold)
        label.alignment = .center
        label.translatesAutoresizingMaskIntoConstraints = false
        let root = NSView(frame: NSRect(x: 0, y: 0, width: 480, height: 320))
        root.addSubview(label)
        NSLayoutConstraint.activate([
            label.centerXAnchor.constraint(equalTo: root.centerXAnchor),
            label.centerYAnchor.constraint(equalTo: root.centerYAnchor),
        ])
        view = root
        preferredContentSize = NSSize(width: 480, height: 320)
    }

    nonisolated public func createAudioUnit(with desc: AudioComponentDescription) throws -> AUAudioUnit {
        try AudioCurveAudioUnit(componentDescription: desc, options: [])
    }
}
