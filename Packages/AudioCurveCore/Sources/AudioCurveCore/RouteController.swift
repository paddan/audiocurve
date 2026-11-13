import Foundation

/// One built system-wide route. The controller tears it down in this order:
/// stop IO, destroy the aggregate device, destroy the tap (which unmutes the system).
public struct BuiltRoute {
    public var stopIO: () -> Void
    public var destroyAggregate: () -> Void
    public var destroyTap: () -> Void

    public init(stopIO: @escaping () -> Void, destroyAggregate: @escaping () -> Void, destroyTap: @escaping () -> Void) {
        self.stopIO = stopIO
        self.destroyAggregate = destroyAggregate
        self.destroyTap = destroyTap
    }
}

@MainActor
public protocol RouteBuilding: AnyObject {
    /// Creates tap, aggregate device and IOProc for the output device and starts IO.
    func build(deviceUID: String) throws -> BuiltRoute
}

/// Owns the system-wide route and keeps it matching `routeTarget(for: inputs)`.
/// Every path that leaves `.running` tears the route down first, so the Mac is never left muted.
@MainActor
public final class RouteController {
    public static let coalesce: TimeInterval = 0.2
    public static let retryDelay: TimeInterval = 0.5
    public static let deathWindow: TimeInterval = 10

    /// Setting it schedules one apply after `coalesce`; several changes in that time give one apply.
    public var inputs = RouteInputs(enabled: false, permission: .unknown, plugin: .loading,
                                    outputUID: nil, activeOutputs: ActiveOutputs()) {
        didSet { if inputs != oldValue { scheduleApply() } }
    }
    public private(set) var state: RouteTarget = .off
    public var onStateChange: ((RouteTarget) -> Void)?
    public var onReloadPlugin: (() -> Void)?

    private let builder: RouteBuilding
    private let now: () -> Date
    private let after: (TimeInterval, @escaping @MainActor () -> Void) -> Void
    private var route: (uid: String, parts: BuiltRoute)?
    private var applyPending = false
    private var forceRebuild = false
    private var lastDeath: Date?
    private var isShutDown = false
    /// Bumped on every apply, so a retry scheduled for an older apply does nothing.
    private var generation = 0

    public init(builder: RouteBuilding,
                now: @escaping () -> Date = Date.init,
                after: @escaping (TimeInterval, @escaping @MainActor () -> Void) -> Void) {
        self.builder = builder
        self.now = now
        self.after = after
    }

    /// Forces a teardown and rebuild after `coalesce`, even if the target is unchanged
    /// (the output device's sample rate changed).
    public func invalidate() {
        forceRebuild = true
        scheduleApply()
    }

    /// The render loop reported a failed render (plugin process gone). Tears down at once and
    /// asks for a reload, unless the previous death was within `deathWindow`.
    public func pluginDied() {
        guard !isShutDown else { return }
        tearDown()
        let t = now()
        let repeated = lastDeath.map { t.timeIntervalSince($0) < Self.deathWindow } ?? false
        lastDeath = t
        setInputsWithoutScheduling { $0.plugin = repeated ? .failed : .loading }
        apply()
        if !repeated { onReloadPlugin?() }
    }

    /// Tears down at once (app quitting). Later input changes do nothing.
    public func shutDown() {
        tearDown()
        isShutDown = true
        setState(.off)
    }

    private func setInputsWithoutScheduling(_ change: (inout RouteInputs) -> Void) {
        let pending = applyPending
        applyPending = true          // suppress scheduling from didSet
        change(&inputs)
        applyPending = pending
    }

    private func scheduleApply() {
        guard !applyPending, !isShutDown else { return }
        applyPending = true
        after(Self.coalesce) { [weak self] in
            guard let self else { return }
            self.applyPending = false
            self.apply()
        }
    }

    private func apply() {
        guard !isShutDown else { return }
        generation += 1
        let target = routeTarget(for: inputs)
        let force = forceRebuild
        forceRebuild = false
        if case .running(let uid) = target, route?.uid == uid, !force {
            setState(target)
            return
        }
        tearDown()
        if case .running(let uid) = target {
            build(uid, retry: true, generation: generation)
        } else {
            setState(target)
        }
    }

    private func build(_ uid: String, retry: Bool, generation g: Int) {
        do {
            route = (uid, try builder.build(deviceUID: uid))
            setState(.running(deviceUID: uid))
        } catch {
            guard retry else { setState(.failed(.route)); return }
            setState(.starting)   // no route while the retry is pending
            after(Self.retryDelay) { [weak self] in
                guard let self, !self.isShutDown, self.generation == g, self.route == nil else { return }
                self.build(uid, retry: false, generation: g)
            }
        }
    }

    private func tearDown() {
        guard let r = route else { return }
        route = nil
        r.parts.stopIO()
        r.parts.destroyAggregate()
        r.parts.destroyTap()
    }

    private func setState(_ s: RouteTarget) {
        guard s != state else { return }
        state = s
        onStateChange?(s)
    }
}
