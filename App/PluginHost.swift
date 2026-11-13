import AudioCurveCore
import AVFoundation
import AppKit
import CoreAudioKit

/// Loads the AudioCurve plugin out of process and keeps its parameter state between launches.
@MainActor
final class PluginHost {
    static let description = AudioComponentDescription(
        componentType: kAudioUnitType_Effect,
        componentSubType: 0x68636D70, // 'hcmp'
        componentManufacturer: 0x4C6E6473, // 'Lnds'
        componentFlags: 0, componentFlagsMask: 0)
    static let stateKey = "pluginState"

    private(set) var unit: AVAudioUnit?
    private(set) var loadState: PluginLoadState = .loading { didSet { onLoadStateChange?(loadState) } }
    var onLoadStateChange: ((PluginLoadState) -> Void)?

    /// The plugin's AUAudioUnit, or nil while it is not loaded.
    var audioUnit: AUAudioUnit? { unit?.withAUAudioUnit { $0 } }

    func load() async {
        loadState = .loading
        do {
            let u = try await AVAudioUnit.instantiate(with: Self.description, options: .loadOutOfProcess)
            if let data = UserDefaults.standard.data(forKey: Self.stateKey),
               let state = try? PropertyListSerialization.propertyList(from: data, format: nil) as? [String: Any] {
                u.withAUAudioUnit { $0.fullState = state }
            }
            unit = u
            loadState = .loaded
        } catch {
            unit = nil
            loadState = .failed
        }
    }

    /// Marks the plugin as loading at once, before an async reload starts.
    func markLoading() { loadState = .loading }

    func reload() async {
        unit = nil
        await load()
    }

    /// Saves the parameter state (not the channel status) to UserDefaults.
    func saveState() {
        guard var state = audioUnit?.fullState else { return }
        state.removeValue(forKey: PluginChannel.statusKey)
        if let data = try? PropertyListSerialization.data(fromPropertyList: state, format: .binary, options: 0) {
            UserDefaults.standard.set(data, forKey: Self.stateKey)
        }
    }

    var bypass: Bool {
        get { audioUnit?.shouldBypassEffect ?? false }
        set { audioUnit?.shouldBypassEffect = newValue }
    }

    func viewController() async -> NSViewController? {
        guard let au = audioUnit else { return nil }
        return await withCheckedContinuation { cont in
            au.requestViewController { cont.resume(returning: $0) }
        }
    }
}

/// Status and commands through the plugin's `fullState` (see `PluginChannel`).
@MainActor
final class PluginClient {
    private let host: PluginHost
    init(host: PluginHost) { self.host = host }

    func status() -> PluginStatus? { PluginChannel.status(from: host.audioUnit?.fullState) }
    func send(_ command: PluginCommand) { host.audioUnit?.fullState = PluginChannel.commandState(command) }
}
