/// How the app behaves: a background menu bar app that processes all system audio, or an
/// ordinary app that processes sound only while its window is open and Bypass is off.
public struct AppMode: Equatable, Sendable {
    public var background: Bool

    public init(background: Bool) { self.background = background }

    /// In background mode the Dock icon shows only while the main window is open.
    public func showsDockIcon(windowOpen: Bool) -> Bool { !background || windowOpen }

    /// Outside background mode the sound plays unprocessed until the user unticks Bypass.
    public var bypassAtStart: Bool { !background }

    /// An ordinary app quits when its window closes.
    public var quitsWhenWindowCloses: Bool { !background }

    /// Launching at login only makes sense for the background app.
    public var offersLaunchAtLogin: Bool { background }
}
