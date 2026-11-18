# Manual tests

UI names below are the English ones; in other languages, use the matching translation.

## Task 2 – passthrough

- [ ] An audio file plays in the host app (AudioCurve.app), and A/B (bypass) makes no audible difference.
- [ ] SoundSource: add the "AudioCurve" effect to system audio; sound passes through without crackles.
- [ ] SoundSource: add the effect to a single app; sound passes through without crackles.
- [ ] If the plugin does not show up or refuses to load: note SoundSource's error message.

## Task 9 – DSP

- [ ] With your audiogram, the treble is lifted clearly more in the ear with the larger high-frequency loss.
- [ ] Moving Strength from 0 to 100 during playback gives no clicks.
- [ ] Output level −10 dB lowers the level audibly.
- [ ] Music mode gives less pumping on music than Speech mode.
- [ ] If the host can force a mono bus: it is refused without a crash.

## Task 10 – UI

- [ ] "Add audiogram" creates a flat curve (0 dB HL, both ears).
- [ ] The audiogram can be entered with the keyboard alone: click left 125 Hz, type "15", Enter, →, "20", Enter and so on, and Tab for the right ear.
- [ ] Dragging with the mouse changes values in 5 dB steps, and in 1 dB steps with ⌥.
- [ ] ⌫ with 4 points left shows the error "At least 4 points per ear".
- [ ] A trim in the Gain view is audible (drag, or ↑/↓ in 1 dB steps; double-click resets).
- [ ] Auto level evens out the A/B level.
- [ ] VoiceOver (⌘F5) reads out and adjusts a point.
- [ ] Dark and light mode both look right.
- [ ] Changes to Strength, Output level, Ceiling or Mode from the host (automation, preset) show up in the UI.

## Spectrum

- [ ] Playback shows in both In and Out.
- [ ] With a high-frequency loss, Out sits above In in the treble.
- [ ] Bypass makes the curves equal.
- [ ] L/R/Both switches the view.
- [ ] The Ceiling line follows the Ceiling (MPO) slider.
- [ ] The L/R/Both buttons do not flicker when the mouse moves over the window during playback.

## Auto mode (speech/music)

- [ ] Mode Auto with a podcast or a news programme: "Auto: speech" within about 10 s.
- [ ] Switch to music: "Auto: music" within about 10 s, with no audible jump in the sound.
- [ ] A pause or track change (silence) does not change the decision.

## Sound quality (2.1)

- [ ] Listen to the A/B WAV files: render them with `cargo run --release --example measure -- --wav` in `dsp/` (files in `dsp/measurements/wav/*_out.wav`), and make the 2.0 reference the same way from a 2.0-equivalent tree: `git worktree add <tmp> 070aef8`, then `cd <tmp>/dsp && cargo run --release --example measure -- --wav`. Compare the same-named `*_out.wav` files: the 2.1 output is cleaner than the 2.0 one. Remove the reference afterwards with `git worktree remove --force <tmp>`.
- [ ] Dense rock or orchestral music at high volume on AirPods: no audible distortion, and the **Safety** meter moves less than in 2.0.
- [ ] A speech podcast and a film with music in **Auto**: it moves between the prescriptions without audible jumps.
- [ ] **Tone** from Clear to Warm: the sound gets fuller and softer, and soft consonants stay audible.
- [ ] Gain view: on each band's gain bar a solid tick marks the soft gain of the speech prescription and a dotted tick the music prescription (the legend below the chart shows both, and the dashed marker is still the loud gain). The two differ (speech higher in the mid and treble), and both move with Strength and the per-band trim.
- [ ] **Tone** slider works in Speech, Music and Auto: it changes the sound in each mode, and it is back to neutral at Clear.

## Safety (limiter)

- [ ] Input level −10 dB: the In meter drops 10 dB and Out drops less than 10 dB (the compression works less).
- [ ] Input level is saved with the host's project and resets to 0 dB on double-click.
- [ ] The Safety meter shows reduction when the music is loud and goes towards 0 when Output level is lowered.
- [ ] The Safety meter shows "off" when the limiter is off.
- [ ] Untick Limiter: the warning dialog appears; Cancel keeps the limiter on, Turn off turns it off.
- [ ] Tick Limiter again: no dialog.

## Final check (spec §7 and §8)

- [ ] SoundSource: change the output device while audio plays; the sound continues without crackles or silence (§7.4).
- [ ] SoundSource: restart SoundSource; the profile (audiogram, trims) is still in the plugin (§7.5).
- [ ] Audio MIDI Setup: set the output device to 44.1 kHz and then 96 kHz; the sound is correct and free of crackles at both (§7.6).
- [ ] 10 minutes of listening to music and speech without crackles, dropouts or peaks (§7.7).
- [ ] 16 kHz (AirPods in call mode): sound passes through without a loud thump and does not go silent; check that the treble above about 7 kHz is missing but speech is audible.
- [ ] Parameter values (Strength, Output level, Ceiling, Mode) survive a restart of the host or SoundSource.
- [ ] A/B with a strong profile: toggling Bypass gives a clearly audible difference (bypass works), and leaving bypass gives no thump.
- [ ] SoundSource passthrough load (§8): the effect on system audio over a longer period gives no disturbing CPU load and no dropouts.
- [ ] The host app loops the file until Stop, and errors at start-up show in the status line.

## Active outputs

- [ ] With every box in the "Active on" dropdown ticked (default), processing works on every output device.
- [ ] Untick every device except the AirPods (the last ticked box cannot be unticked): with the AirPods as output, processing is on; switch the output to the hearing aids (or turn the AirPods off) and the sound passes through unchanged, with "Paused – <device> is the output" shown.
- [ ] The switch is a smooth fade, without a click or a gap.
- [ ] A remembered device that is not connected shows "(not connected)" in the menu.
- [ ] The selection is shared by every instance (host app and SoundSource) and survives a restart.

## System-wide app (2.0)

Before testing a Debug build, make sure its plugin is the registered one (see the developer note in `docs/superpowers/plans/2026-11-13-spike-notes.md`).

- [ ] First launch: the window opens, the ear appears in the menu bar and the app asks once whether to start at login.
- [ ] First processing: macOS asks for permission to record system audio. After Allow, sound is processed.
- [ ] AirPods as output: music, a video and a call app are processed. Bypass (A/B) gives an audible difference.
- [ ] Switch to the hearing aids and back: "Paused – … is the output", then "Active – …", with no click and no lasting silence.
- [ ] Deny the permission (`tccutil reset AudioCapture se.lindefors.audiocurve`, then Don't Allow): within a few seconds the status asks for the permission, and the sound plays unprocessed. Open System Settings works. Allow it, turn AudioCurve off and on: processing resumes.
- [ ] Kill the `AudioCurveAU` process while audio plays: the sound continues unprocessed within a second, then processing resumes.
- [ ] Output at 44.1, 48 and 96 kHz (Audio MIDI Setup): sound correct at each, including a change while playing.
- [ ] Latency: a video with speech has no visible lip-sync error. Note the measured figure (loopback recording): ____ ms.
- [ ] One hour of mixed listening without dropouts.
- [ ] SoundSource also running AudioCurve on the same output: the panel shows the "processed twice" warning within a few seconds.
- [ ] Quit the app while audio plays: the sound continues unprocessed.
- [ ] The panel's Profile picker and Active on match the window, in both directions.
- [ ] The Dock icon shows only while the window is open (background mode).
- [ ] Launch at login (Settings): tick it, log out and in: the ear is back and the window stays closed.

## Profiles

- [ ] The audiogram is still there after restarting the host app.
- [ ] Save As, load, Rename, Duplicate and Delete work from the profile menu in the header.
- [ ] The unsaved dot (•) appears after a change and disappears on Save.
- [ ] The same versions show up in SoundSource.

## Profile per output (2.2)

Needs two saved profiles, A and B, and two outputs (here the speakers and the AirPods). Assign with the profile pop-ups under "Active on" in the menu panel (or "Use on <output>" in the window's profile menu).

- [ ] Assign A to the speakers and B to the AirPods. Switching the system output loads A on the speakers and B on the AirPods; the panel and the window show the loaded profile.
- [ ] Assigning a profile to the current output loads it at once.
- [ ] On the speakers (A), change the audiogram in the editor, then switch to the AirPods and back: the edits are in A (saved into A, no "Unsaved" profile appears), and B is unchanged.
- [ ] With a never-saved profile that is not flat loaded, switch to an output with an assigned profile: a profile named "Unsaved – <date time>" appears in the list holding those edits.
- [ ] Set different Strength, Tone, Input level, Output level and Maximum output on each output: switching back and forth brings each output's own levels back, also after restarting the app. Mode, Compression, Limiter and Bypass do not change on a switch.
- [ ] An output without an assignment keeps the current profile and levels when switched to.
- [ ] Loading a profile by hand on an output does not change its assignment; the next switch to that output loads the assigned profile again.
- [ ] SoundSource running AudioCurve: switching the output does not change SoundSource's profile or levels, and no "Unsaved" profile is created by it.
- [ ] Delete B: switching to the AirPods keeps the current profile (no error); the AirPods' pop-up shows "—".

## Hearing test (2.2)

Run with AirPods (or other headphones with a settable volume). Before each test, set the volume to max by hand once and check the first tones: none may sound unexpectedly loud.

- [ ] Full test on AirPods: "Hearing test…" shows the intro with "Output: AirPods …", the checklist (including the Do Not Disturb and system-wide EQ lines) and "In-ear" preselected. Start: the volume goes to max, tones come right ear first, then left; the progress counts to 24. Space and the "I hear it" button both answer; holding Space does not answer the next tone.
- [ ] No tone sounds unexpectedly loud (start with the volume already at max to compare); tones step up gently above 80 dB HL.
- [ ] At the end, the volume is back where it was, the result shows both ears (right red circles, left blue crosses), "Relative to <device>", any flags as warnings, and the name "<device> – <date>" with "Use on <device>" ticked.
- [ ] Save: the window goes back to the plugin; the profile is in the profile list, assigned to the AirPods and active.
- [ ] Pause: the volume goes back to where it was at once. Resume: the volume goes to max again and the same tone is played again.
- [ ] While paused, play music in another app: the paused screen says to stop it and Resume is disabled; stop the music and Resume works. Same with muting the output while paused.
- [ ] Force-quit the app mid-test (Activity Monitor): on the next launch the volume is restored.
- [ ] Switch the output mid-test: the test stops at once with a message, the volume of the old output is restored.
- [ ] Disconnect the headphones mid-test (Bluetooth off or AirPods in the case), then reconnect them with the app still running: their volume is restored (not left at max).
- [ ] Disconnect the AirPods mid-test, run a test on another output, then reconnect the AirPods: the AirPods get their own original volume back.
- [ ] Close the window mid-test: the test stops and the volume is restored; reopening shows the intro with a message.
- [ ] Quit from the menu mid-test: the volume is restored.
- [ ] SoundSource running AudioCurve: the intro names it and Start is disabled; starting it during a test stops the test.
- [ ] Play music in another app: the intro warns and Start is disabled; started during the test, the test pauses, the volume goes back down at once, and Resume (after stopping the music) raises it again.
- [ ] Mute the output: the intro says to unmute it and Start is disabled.
- [ ] An output without a settable volume (e.g. HDMI): the intro says to set the volume to max yourself and to turn it back down when the test ends (no "The volume is restored afterwards"); the test runs. The result screen says "Turn the volume back down now."; so does the intro after Cancel mid-test (Cancel goes back to the intro instead of closing) and after a stopped test.
- [ ] AirPods (or a Bluetooth headset) in call mode (open a call or the Voice Memos recorder so its microphone is used, the output drops to 16 kHz): the intro says the output runs at a low sample rate and Start is disabled. Starting the microphone during a test stops it with a message.
- [ ] The status line says "Hearing test in progress" during the test, and the route resumes afterwards.

## Launch and settings (2.3)

Defaults: **Process all sound in the background** and **Show in menu bar** on.

- [ ] Launch from Finder, the Dock, Launchpad and Spotlight: the window opens each time.
- [ ] Close the window (background mode): the ear stays, the Dock icon goes, the sound is still processed. Open the app from its icon again: the window opens.
- [ ] With another app in front and the window closed: **Settings…** in the panel opens Settings in front, with a Dock icon until Settings closes. ⌘, in the window opens it too; closing the main window while Settings is open keeps Settings reachable.
- [ ] Minimise the window, then change a setting: the Dock icon and the minimised window stay.
- [ ] Untick **Show in menu bar**: the ear disappears at once and the hint about the app icon shows. Close the window: the app keeps running without ear or Dock icon. Double-click the app in Finder: the window opens. Tick it again: the ear is back.
- [ ] Untick **Process all sound in the background** with the window open: Bypass ticks itself, the sound plays unprocessed, **Launch at login** disappears from Settings and is off in System Settings → General → Login Items.
- [ ] With the main window closed (background mode), untick **Process all sound in the background** in Settings: the main window opens.
- [ ] In that mode: untick Bypass: the sound is processed. Tick it again: unprocessed, with no gap.
- [ ] In that mode: close Settings: the app keeps running. Close the main window: the app quits and the sound continues unprocessed.
- [ ] In that mode: launch again: the window opens with Bypass ticked and the Dock icon shows.
- [ ] In that mode, with a hearing test running: close the window: the app quits and the headphone volume is restored.
- [ ] In that mode, untick Bypass and kill the `AudioCurveAU` process: after the reload Bypass is still unticked and the sound is processed again.
- [ ] Tick **Process all sound in the background** again: Bypass unticks itself and the sound is processed; closing the window no longer quits.
- [ ] Both settings survive a relaunch.
- [ ] Settings in each language: the toggles and the explanations are translated and fit.

## Languages

The plugin and the host app are available in English, Swedish, German, Spanish, French, Finnish, Norwegian (Bokmål) and Danish. Other languages fall back to English. To start the host app in a given language:

```bash
open -n build/dd/Build/Products/Debug/AudioCurve.app --args -AppleLanguages '(de)'
```

Replace `de` with `en`, `sv`, `es`, `fr`, `fi`, `nb` or `da`.

- The plugin runs in its own process, which the system starts with the host's language list (`-AppleLanguages`). Check that the plugin's text switches language together with the host app.
- In SoundSource and other hosts, the plugin follows their language, usually the system language (System Settings → General → Language & Region).
- Do not run two builds of the app at the same time, or the wrong version of the plugin may load.

For each language:

- [ ] Host app: the buttons (Open Audio File…, Play/Stop, Bypass) and the status line (Loading plugin…, Plugin loaded, error messages) are translated.
- [ ] The view picker (Audiogram/Gain/Spectrum), the axis caption at the top right and the empty state (No audiogram, Add audiogram) are translated.
- [ ] Controls panel: Mode (Speech/Music/Auto), Strength, Input level, Output level, Ceiling (MPO), Compression, Limiter, Auto level and its status text each fit on one line at the minimum window width (640). Check German, Spanish, French and Finnish in particular.
- [ ] Meters: the In/Out/Safety labels fit and the bars start in the same column; "off" appears translated when the limiter is off.
- [ ] Untick Limiter: the warning dialog's title, text and buttons are translated.
- [ ] Profile menu: Unsaved profile, Save, Save As…, Rename…, Duplicate, Delete… and the dialogs are translated. Save As suggests "My profile" in the right language, and Duplicate gives "<name> copy" in the right language.
- [ ] Error messages (⌫ with 4 points, an invalid typed value, a name already taken) appear translated.
- [ ] Spectrum view: L/R/Both (e.g. L/R/Beide, G/D/Les deux, V/O/Molemmat) and the Ceiling line label are translated.
- [ ] Gain view: the legend at the bottom right and the label of the selected bar are translated.
- [ ] VoiceOver (⌘F5): audiogram points, Gain bars, the meters and Safety are read out in the right language.
- [ ] Hovering over the Safety meter shows translated help text.
- [ ] In a DAW or host that shows parameter names: Strength, Ceiling, Input level, Output level, Mode (Speech/Music/Auto), Compression and Limiter are translated.
- [ ] The decimal separator follows the region (comma in e.g. Sweden or Germany, point with the US region).
- [ ] An unsupported language (e.g. `-AppleLanguages '(it)'`) gives English.

Known: the plugin's name in the hosts' effect lists comes from Info.plist and is not translated.
