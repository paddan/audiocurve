# Opening from the app icon, background mode and the menu bar setting

Date: 2026-11-18
Status: approved approach, short design

## Intent

Today AudioCurve.app is a pure menu bar app: launching it from Finder, the Dock, Launchpad or Spotlight shows only the ear. The user wants:

- the window to open when the app is launched from its icon;
- a setting for whether AudioCurve runs as a **background app that processes all system audio**;
- a setting for whether the **ear is shown in the menu bar**.

## Behaviour (decided with the user)

**Process system audio in the background** (default on, as today):

- On: as today. AudioCurve keeps running and processing with the window closed. Bypass is turned off when the app launches and when the setting is turned on.
- Off: AudioCurve is an ordinary app. It processes system audio only while the user has unticked **Bypass (A/B)** in the window; at launch Bypass is ticked, so the sound plays as normal until the user chooses otherwise. **Closing the window quits the app** and the sound plays normally again.

**Show in menu bar** (default on): shows or hides the ear at once, with no restart.

**Opening the window:**

- Launched from the icon: the window opens, in both modes.
- Opened again while running (Dock click, Finder double-click): the window opens. This is the way back when the ear is hidden.
- Launched as a login item: stays hidden, as today.

## Approaches

1. **The main window is owned by AppKit** (picked). The app delegate owns an `NSWindow` with an `NSHostingController(HostView)` and shows it at launch, on reopen (`applicationShouldHandleReopen`) and from **Open AudioCurve…**. A login launch is recognised by `keyAELaunchedAsLogInItem` in the launch Apple Event. Exact and flicker free; `MainWindow.swift` becomes a small window controller.
2. Keep the SwiftUI `Window` scene and open it via an `openWindow` action captured from a view. Least code, but MenuBarExtra content is not rendered until opened, so it needs a hidden helper view; fragile when the ear is hidden.
3. `.defaultLaunchBehavior(.presented)` and close the window on a login launch. Simple, but the window flashes at login and reopening a running app is not solved.

## Design

- `AppModel.backgroundProcessing` and `AppModel.showInMenuBar`, both `Bool` in `UserDefaults` (keys `backgroundProcessing`, `showInMenuBar`), default `true`.
- `MenuBarExtra(isInserted:)` bound to `showInMenuBar`.
- Bypass: still the plugin's own bypass (instant A/B, the route stays built while the app runs). After the plugin loads, bypass is set to `!backgroundProcessing`. Turning background on sets bypass off; turning it off sets bypass on. `HostView` reads the bypass from the model so the toggle follows.
- Launch at login only belongs to background mode: turning background off unregisters it, and the Settings toggle for it is shown only in background mode. The first-launch question about it is asked only in background mode.
- A `Settings` scene (⌘, and **Settings…** in the menu panel) with the three toggles. The **Launch at login** checkbox in the panel is replaced by **Settings…**.
- Activation policy: `.regular` while the main window is open or in non-background mode; `.accessory` otherwise (as today, the Dock icon only while the window is open). `LSUIElement` stays, so a login launch never shows a Dock icon first.
- Window close in non-background mode: `model.quit()` (unmutes, saves state, terminates). A running hearing test is ended by the normal shutdown path.

- Builder's calls: the app is `.regular` while Settings is open (so Settings comes to the front from the panel); a minimised main window counts as open; turning background off while the main window is closed opens it; `AUDIOCURVE_DEMO` launches keep the window closed.

## Non-goals

- No change to the route, the plugin or `RouteController`.
- No processing that is tied to window visibility in background mode.

## Left to the builder

Exact strings; whether Settings is also reachable from the window (⌘, covers it); edge cases with the Settings window when the main window closes (only the main window's close quits).
