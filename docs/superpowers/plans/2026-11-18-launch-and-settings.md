# Opening from the app icon and background/menu bar settings – Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Launching AudioCurve.app from its icon opens the window; settings choose background processing and the menu bar ear.

**Architecture:** A pure `AppMode` value in AudioCurveCore decides the activation policy, the bypass at launch and whether closing the window quits. The app delegate owns the main `NSWindow` (AppKit) and a SwiftUI `Settings` scene holds the toggles.

**Tech Stack:** Swift 6, SwiftUI, AppKit, ServiceManagement, Swift Testing/XCTest as the package already uses.

**Spec:** `docs/superpowers/specs/2026-11-18-launch-and-settings-design.md`

## Global Constraints

- Views use `@ViewState`, never `@State`.
- New App files go directly in `App/`; run `xcodegen generate` after adding one.
- Every new UI string has da, de, es, fi, fr, nb, sv translations in `App/Localizable.xcstrings`.
- Defaults: `backgroundProcessing = true`, `showInMenuBar = true` (UserDefaults keys `backgroundProcessing`, `showInMenuBar`).
- `LSUIElement` stays `true`.
- Commits: author `19973950+paddan@users.noreply.github.com`, conventional prefixes, last line `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`. No push.

## Review Focus

- Ear hidden, background on, window closed: Dock click / Finder double-click must open the window (`applicationShouldHandleReopen`).
- Non-background: closing the Settings window must not quit; only the main window's close quits.
- Non-background: quitting via window close while a hearing test runs restores the volume (normal `shutDown`).
- Turning background off while the window is open: bypass becomes on, Dock icon stays, login item unregistered.
- Plugin reload (crash recovery) must re-apply the bypass for the current mode, not leave it at the AU default.

---

### Task 1: `AppMode` policy in AudioCurveCore

**Files:**
- Create: `Packages/AudioCurveCore/Sources/AudioCurveCore/AppMode.swift`
- Test: `Packages/AudioCurveCore/Tests/AudioCurveCoreTests/AppModeTests.swift`

**Interfaces:**
- Produces: `public struct AppMode: Equatable, Sendable { public var background: Bool; init(background:) }` with
  - `func showsDockIcon(windowOpen: Bool) -> Bool` — `!background || windowOpen`
  - `var bypassAtStart: Bool` — `!background`
  - `var quitsWhenWindowCloses: Bool` — `!background`
  - `var offersLaunchAtLogin: Bool` — `background`

- [ ] Write tests: background → no Dock icon when closed, Dock icon when open, bypass false, no quit, offers login; non-background → Dock icon in both, bypass true, quits, no login.
- [ ] Run `swift test --filter AppModeTests` in `Packages/AudioCurveCore`: fails (type missing).
- [ ] Implement.
- [ ] Run again: passes. Commit `feat(core): app mode policy for background processing`.

### Task 2: Settings in the model, main window in AppKit, Settings scene

**Files:**
- Modify: `App/AppModel.swift`, `App/AudioCurveApp.swift`, `App/MainWindow.swift`, `App/MenuPanel.swift`, `App/HostView.swift`, `App/Localizable.xcstrings`
- Create: `App/SettingsView.swift`

**Interfaces:**
- Consumes: `AppMode`.
- Produces:
  - `AppModel.backgroundProcessing: Bool` (persisted; `didSet`: bypass = `mode.bypassAtStart`, unregister login if off, `onModeChange?()`), `AppModel.showInMenuBar: Bool` (persisted), `var mode: AppMode`, `var onModeChange: (() -> Void)?`.
  - `AppModel.bypass` becomes stored-and-observable: `private(set) var bypassOn` mirror, setter writes `host.bypass`. `afterLoad()` sets `bypass = mode.bypassAtStart`.
  - `MainWindowController` (in `MainWindow.swift`): `init(model:)`, `func show()` (create on first use, `makeKeyAndOrderFront`, `NSApp.activate()`), `var isOpen: Bool`, close via `NSWindowDelegate.windowWillClose` → `onClose?()`. Window: title "AudioCurve", autosave name `main`, `NSHostingController(rootView: HostView(model:))`, `isReleasedWhenClosed = false`.
  - `AppDelegate.showMainWindow()`, `applyActivationPolicy()` (uses `mode.showsDockIcon(windowOpen:)`), `applicationShouldHandleReopen` → `showMainWindow()`; `applicationDidFinishLaunching` shows the window unless launched as a login item (`NSAppleEventManager.shared().currentAppleEvent?.paramDescriptor(forKeyword: keyAEPropData)?.enumCodeValue == keyAELaunchedAsLogInItem`). Window close: `mode.quitsWhenWindowCloses ? model.quit() : applyActivationPolicy()`; also call `model.hearingTest?.windowClosed()` there (moved from `HostView.onDisappear`).
  - `MenuBarExtra(isInserted: $model.showInMenuBar)`; `Settings { SettingsView(model:) }`.
  - Menu panel: **Open AudioCurve…** calls an injected `openMain: () -> Void` (from the delegate); **Launch at login** checkbox replaced by **Settings…** (`SettingsLink`, then `NSApp.activate()`).
  - `SettingsView`: `Form` with toggles **Process system audio in the background** (+ footnote), **Show in menu bar**, and **Launch at login** only when `mode.offersLaunchAtLogin`.
  - First-launch login question only when `mode.offersLaunchAtLogin`; text says "You can change this later in Settings."

- [ ] Implement; `xcodegen generate`.
- [ ] `make release` and `xcodebuild -scheme audiocurve -configuration Debug build` succeed; `make` rejects no `@State`.
- [ ] Add translations for every new/changed string.
- [ ] Commit `feat(app): open the window from the app icon; settings for background processing and the menu bar`.

### Task 3: Docs

**Files:** `docs/manual-test.md` (new section "Launch and settings (2.3)"), `README.md` (Download step 4, Using it, Features).

- [ ] Write the manual-test checklist covering the spec's behaviour and the Review Focus.
- [ ] Update README.
- [ ] Commit `docs: launch from the app icon, background and menu bar settings`.
