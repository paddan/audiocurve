import SwiftUI

/// SwiftUI's `State` property wrapper under a name that does not collide with the
/// `@State` macro of the macOS 27 SDK. That macro needs the SwiftUIMacros compiler
/// plugin, which ships with Xcode but not with the Command Line Tools, so views use
/// `@ViewState` to stay buildable with `make release` (see README, "Building without Xcode").
typealias ViewState = SwiftUI.State
