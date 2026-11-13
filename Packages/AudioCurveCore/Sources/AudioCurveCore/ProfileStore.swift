import Foundation

public struct SavedProfile: Codable, Equatable, Identifiable, Sendable {
    public var id: UUID
    public var name: String
    public var modified: Date
    public var profile: HearingProfile

    public init(id: UUID, name: String, modified: Date, profile: HearingProfile) {
        self.id = id
        self.name = name
        self.modified = modified
        self.profile = profile
    }
}

public struct WorkingCopy: Codable, Equatable, Sendable {
    public var profile: HearingProfile
    /// The saved version it was loaded from / last saved to.
    public var sourceID: UUID?

    public init(profile: HearingProfile, sourceID: UUID?) {
        self.profile = profile
        self.sourceID = sourceID
    }
}

public enum ProfileStoreError: Error, Equatable {
    case emptyName, nameTaken, notFound, io(String)
}

/// Named profile versions plus an autosaved working copy, as JSON files under `root`.
/// Main-thread use; keeps no cache because other plugin instances may write the same directory.
public final class ProfileStore: @unchecked Sendable {
    public static let fileVersion = 1
    public static let maxNameLength = 60

    private struct Envelope<T: Codable>: Codable {
        var version: Int
        var value: T
    }

    private let root: URL
    private var profilesDir: URL { root.appendingPathComponent("profiles", isDirectory: true) }
    private var workingCopyURL: URL { root.appendingPathComponent("current.json") }
    private var activeOutputsURL: URL { root.appendingPathComponent("outputs.json") }
    private var instancesDir: URL { root.appendingPathComponent("instances", isDirectory: true) }

    public init(root: URL) { self.root = root }

    public static func standard() -> ProfileStore {
        let base = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first
            ?? FileManager.default.temporaryDirectory
        return ProfileStore(root: base.appendingPathComponent("audiocurve", isDirectory: true))
    }

    // MARK: Versions

    public func list() -> [SavedProfile] {
        let urls = (try? FileManager.default.contentsOfDirectory(at: profilesDir, includingPropertiesForKeys: nil)) ?? []
        return urls.filter { $0.pathExtension == "json" }
            .compactMap { read(SavedProfile.self, from: $0) }
            .filter { (try? $0.profile.validate()) != nil }
            .sorted { $0.name.localizedStandardCompare($1.name) == .orderedAscending }
    }

    public func saved(id: UUID) -> SavedProfile? {
        guard let s = read(SavedProfile.self, from: url(for: id)), (try? s.profile.validate()) != nil else { return nil }
        return s
    }

    @discardableResult
    public func saveNew(name: String, profile: HearingProfile) throws(ProfileStoreError) -> SavedProfile {
        try checkValid(profile)
        let clean = try cleanName(name, excluding: nil)
        let s = SavedProfile(id: UUID(), name: clean, modified: Date(), profile: profile)
        try write(s, to: url(for: s.id))
        return s
    }

    public func overwrite(id: UUID, profile: HearingProfile) throws(ProfileStoreError) {
        try checkValid(profile)
        guard var s = saved(id: id) else { throw .notFound }
        s.profile = profile
        s.modified = Date()
        try write(s, to: url(for: id))
    }

    public func rename(id: UUID, to name: String) throws(ProfileStoreError) {
        guard var s = saved(id: id) else { throw .notFound }
        s.name = try cleanName(name, excluding: id)
        s.modified = Date()
        try write(s, to: url(for: id))
    }

    /// Name for the `n`th copy of `base` (n = 1 for the first). The package has no resources, so the
    /// UI passes a localized one; this default is the English source.
    public static func defaultCopyName(_ base: String, _ n: Int) -> String {
        n == 1 ? "\(base) copy" : "\(base) copy \(n)"
    }

    @discardableResult
    public func duplicate(id: UUID,
                          copyName: (_ base: String, _ n: Int) -> String = ProfileStore.defaultCopyName)
        throws(ProfileStoreError) -> SavedProfile {
        guard let s = saved(id: id) else { throw .notFound }
        let taken = Set(list().map { $0.name.lowercased() })
        var n = 1
        var candidate = copyName(s.name, n)
        while taken.contains(candidate.lowercased()) {
            n += 1
            candidate = copyName(s.name, n)
        }
        return try saveNew(name: candidate, profile: s.profile)
    }

    public func delete(id: UUID) throws(ProfileStoreError) {
        guard FileManager.default.fileExists(atPath: url(for: id).path) else { throw .notFound }
        do { try FileManager.default.removeItem(at: url(for: id)) }
        catch { throw .io(error.localizedDescription) }
    }

    // MARK: Working copy

    public func loadWorkingCopy() -> WorkingCopy? {
        guard let w = read(WorkingCopy.self, from: workingCopyURL), (try? w.profile.validate()) != nil else { return nil }
        return w
    }

    public func saveWorkingCopy(_ w: WorkingCopy) {
        try? write(w, to: workingCopyURL)
    }

    // MARK: Helpers

    // MARK: Active outputs

    /// Missing or corrupt file → all outputs (the default).
    public func loadActiveOutputs() -> ActiveOutputs {
        read(ActiveOutputs.self, from: activeOutputsURL) ?? ActiveOutputs()
    }

    /// Best effort, never throws.
    public func saveActiveOutputs(_ a: ActiveOutputs) {
        try? write(a, to: activeOutputsURL)
    }

    // MARK: Instance heartbeats

    /// Writes `instances/<pid>.json`. Best effort, never throws.
    public func writeHeartbeat(_ b: InstanceHeartbeat) {
        try? write(b, to: instancesDir.appendingPathComponent("\(b.pid).json"))
    }

    /// Every readable heartbeat; unreadable files are skipped.
    public func readHeartbeats() -> [InstanceHeartbeat] {
        let files = (try? FileManager.default.contentsOfDirectory(at: instancesDir, includingPropertiesForKeys: nil)) ?? []
        return files.filter { $0.pathExtension == "json" }
            .compactMap { read(InstanceHeartbeat.self, from: $0) }
            .sorted { $0.pid < $1.pid }
    }

    /// Removes heartbeats older than `InstanceHeartbeat.pruneAfter` (instances that died without
    /// cleaning up). Best effort.
    public func pruneHeartbeats(now: Date) {
        for b in readHeartbeats() where now.timeIntervalSince(b.time) > InstanceHeartbeat.pruneAfter {
            removeHeartbeat(pid: b.pid)
        }
    }

    public func removeHeartbeat(pid: Int32) {
        try? FileManager.default.removeItem(at: instancesDir.appendingPathComponent("\(pid).json"))
    }

    private func url(for id: UUID) -> URL {
        profilesDir.appendingPathComponent("\(id.uuidString).json")
    }

    private func checkValid(_ p: HearingProfile) throws(ProfileStoreError) {
        guard (try? p.validate()) != nil else { throw .io("invalid profile") }
    }

    /// Trims, truncates, and rejects empty or already used names (case-insensitive).
    private func cleanName(_ raw: String, excluding id: UUID?) throws(ProfileStoreError) -> String {
        var name = raw.trimmingCharacters(in: .whitespacesAndNewlines)
        if name.isEmpty { throw .emptyName }
        if name.count > Self.maxNameLength {
            name = String(name.prefix(Self.maxNameLength)).trimmingCharacters(in: .whitespacesAndNewlines)
        }
        let key = name.lowercased()
        if list().contains(where: { $0.id != id && $0.name.lowercased() == key }) { throw .nameTaken }
        return name
    }

    private func read<T: Codable>(_ type: T.Type, from url: URL) -> T? {
        guard let data = try? Data(contentsOf: url) else { return nil }
        let decoder = JSONDecoder()
        guard let env = try? decoder.decode(Envelope<T>.self, from: data), env.version == Self.fileVersion else { return nil }
        return env.value
    }

    private func write<T: Codable>(_ value: T, to url: URL) throws(ProfileStoreError) {
        let encoder = JSONEncoder()
        do {
            try FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
            let data = try encoder.encode(Envelope(version: Self.fileVersion, value: value))
            try data.write(to: url, options: .atomic)
        } catch {
            throw .io(error.localizedDescription)
        }
    }
}
