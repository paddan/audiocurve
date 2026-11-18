import Foundation

/// The level controls remembered per output.
public struct LevelSettings: Codable, Equatable, Sendable {
    public var strength: Double
    public var inputGainDB: Double
    public var outputGainDB: Double
    public var tone: Double
    public var mpoDBFS: Double

    public init(strength: Double, inputGainDB: Double, outputGainDB: Double, tone: Double, mpoDBFS: Double) {
        self.strength = strength
        self.inputGainDB = inputGainDB
        self.outputGainDB = outputGainDB
        self.tone = tone
        self.mpoDBFS = mpoDBFS
    }
}

/// What an output is set up with: a saved profile and/or level settings.
public struct OutputAssignment: Codable, Equatable, Sendable {
    public var device: OutputDevice
    public var profileID: UUID?
    public var levels: LevelSettings?

    public init(device: OutputDevice, profileID: UUID? = nil, levels: LevelSettings? = nil) {
        self.device = device
        self.profileID = profileID
        self.levels = levels
    }
}

/// Per-output assignments, keyed by `OutputDevice.uid`.
public struct OutputAssignments: Codable, Equatable, Sendable {
    public var byUID: [String: OutputAssignment]

    public init(byUID: [String: OutputAssignment] = [:]) { self.byUID = byUID }

    public func assignment(_ uid: String) -> OutputAssignment? { byUID[uid] }

    public mutating func assign(_ d: OutputDevice, profile: UUID?, levels: LevelSettings?) {
        byUID[d.uid] = OutputAssignment(device: d, profileID: profile, levels: levels)
    }

    /// Updates the levels of an already known output; unknown outputs are left alone.
    public mutating func setLevels(_ l: LevelSettings, for uid: String) {
        byUID[uid]?.levels = l
    }

    /// Clears references to deleted profiles and drops entries left with nothing to apply.
    public mutating func prune(existing: Set<UUID>) {
        for (uid, var a) in byUID {
            if let p = a.profileID, !existing.contains(p) { a.profileID = nil }
            if a.profileID == nil && a.levels == nil { byUID[uid] = nil } else { byUID[uid] = a }
        }
    }
}

public enum OutputSwitchAction: Equatable, Sendable {
    case overwrite(UUID, HearingProfile)
    case saveUnsaved(HearingProfile)
    case load(UUID)
    case applyLevels(LevelSettings)
}

/// What to do when the system output changes to one with `assignment`.
public enum OutputSwitch {
    /// - Parameter savedSource: the saved profile of `workingCopy.sourceID`, nil if there is none or it was deleted.
    public static func actions(assignment: OutputAssignment?, workingCopy: WorkingCopy?,
                               savedSource: HearingProfile?, existing: Set<UUID>) -> [OutputSwitchAction] {
        guard let assignment else { return [] }
        var result: [OutputSwitchAction] = []
        let levels = assignment.levels.map { OutputSwitchAction.applyLevels($0) }
        if let target = assignment.profileID, existing.contains(target), target != workingCopy?.sourceID {
            if let wc = workingCopy {
                if let source = wc.sourceID, let saved = savedSource {
                    if saved != wc.profile { result.append(.overwrite(source, wc.profile)) }
                } else if wc.profile != .flat() {
                    result.append(.saveUnsaved(wc.profile))
                }
            }
            // The new output's levels (and ceiling) before its profile, so a stronger profile
            // never runs with the old output's levels.
            if let levels { result.append(levels) }
            result.append(.load(target))
        } else if let levels {
            result.append(levels)
        }
        return result
    }
}
