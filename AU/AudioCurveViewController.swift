import AppKit
import AudioToolbox
import CoreAudioKit
import SwiftUI

public final class AudioCurveViewController: AUViewController, AUAudioUnitFactory {
    private var audioUnit: AudioCurveAudioUnit?
    private var hosting: NSHostingController<PluginView>?

    public override func loadView() {
        view = NSView(frame: NSRect(x: 0, y: 0, width: 720, height: 420))
        preferredContentSize = NSSize(width: 720, height: 420)
    }

    public override func viewDidLoad() {
        super.viewDidLoad()
        buildUIIfReady()
    }

    /// Builds the SwiftUI UI once both the view and the audio unit exist (either may come first).
    private func buildUIIfReady() {
        guard hosting == nil, isViewLoaded, let audioUnit else { return }
        let controller = NSHostingController(rootView: PluginView(vm: PluginViewModel(audioUnit: audioUnit)))
        controller.sizingOptions = [.minSize]
        addChild(controller)
        controller.view.translatesAutoresizingMaskIntoConstraints = false
        view.addSubview(controller.view)
        NSLayoutConstraint.activate([
            controller.view.leadingAnchor.constraint(equalTo: view.leadingAnchor),
            controller.view.trailingAnchor.constraint(equalTo: view.trailingAnchor),
            controller.view.topAnchor.constraint(equalTo: view.topAnchor),
            controller.view.bottomAnchor.constraint(equalTo: view.bottomAnchor),
        ])
        hosting = controller
    }

    private struct AudioUnitBox: @unchecked Sendable { let unit: AudioCurveAudioUnit }

    nonisolated public func createAudioUnit(with desc: AudioComponentDescription) throws -> AUAudioUnit {
        let unit = try AudioCurveAudioUnit(componentDescription: desc, options: [])
        let box = AudioUnitBox(unit: unit)
        DispatchQueue.main.async { [weak self] in
            MainActor.assumeIsolated {
                self?.audioUnit = box.unit
                self?.buildUIIfReady()
            }
        }
        return unit
    }
}
