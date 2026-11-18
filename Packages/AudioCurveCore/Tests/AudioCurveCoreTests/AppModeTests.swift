import Testing
@testable import AudioCurveCore

@Test func backgroundModeIsAMenuBarApp() {
    let m = AppMode(background: true)
    #expect(!m.showsDockIcon(windowOpen: false))
    #expect(m.showsDockIcon(windowOpen: true))
    #expect(!m.bypassAtStart)
    #expect(!m.quitsWhenWindowCloses)
    #expect(m.offersLaunchAtLogin)
}

@Test func foregroundModeIsAnOrdinaryApp() {
    let m = AppMode(background: false)
    #expect(m.showsDockIcon(windowOpen: false))
    #expect(m.showsDockIcon(windowOpen: true))
    #expect(m.bypassAtStart)
    #expect(m.quitsWhenWindowCloses)
    #expect(!m.offersLaunchAtLogin)
}
