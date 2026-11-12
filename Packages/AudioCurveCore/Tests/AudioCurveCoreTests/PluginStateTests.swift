import Foundation
import Testing
@testable import AudioCurveCore

@Test func roundTrip() {
    let state = PluginState(version: PluginState.currentVersion, profile: exampleProfile())
    let decoded = PluginState.decode(state.encoded())
    #expect(decoded == state)
    #expect(decoded.profile != nil)
}

@Test func missingDataYieldsNilProfile() {
    #expect(PluginState.decode(nil).profile == nil)
}

@Test func corruptDataYieldsNilProfile() {
    #expect(PluginState.decode(Data("{".utf8)).profile == nil)
}

@Test func unknownVersionYieldsNilProfile() {
    let state = PluginState(version: 99, profile: exampleProfile())
    #expect(PluginState.decode(state.encoded()).profile == nil)
}

@Test func invalidProfileYieldsNil() {
    var p = HearingProfile.flat()
    p.left = Audiogram(points: [125, 250, 500].map { AudiogramPoint(frequencyHz: $0, thresholdDBHL: 0) })
    let state = PluginState(version: PluginState.currentVersion, profile: p)
    #expect(PluginState.decode(state.encoded()).profile == nil)
}

@Test func resolveKeepsCurrentWhenHostHasNoProfile() {
    let current = exampleProfile()
    #expect(PluginState.resolve(hostProfile: nil, current: current) == current)
    #expect(PluginState.resolve(hostProfile: nil, current: nil) == nil)
}

@Test func resolvePrefersHostProfile() {
    let host = exampleProfile()
    #expect(PluginState.resolve(hostProfile: host, current: .flat()) == host)
    #expect(PluginState.resolve(hostProfile: host, current: nil) == host)
}
