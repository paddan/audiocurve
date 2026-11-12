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

- [ ] With "Active on: all outputs" (default), processing works on every output device.
- [ ] Tick only the AirPods: with the AirPods as output, processing is on; switch the output to the hearing aids (or turn the AirPods off) and the sound passes through unchanged, with "Paused – <device> is the output" shown.
- [ ] The switch is a smooth fade, without a click or a gap.
- [ ] A remembered device that is not connected shows "(not connected)" in the menu.
- [ ] The selection is shared by every instance (host app and SoundSource) and survives a restart.

## Profiles

- [ ] The audiogram is still there after restarting the host app.
- [ ] Save As, load, Rename, Duplicate and Delete work from the profile menu in the header.
- [ ] The unsaved dot (•) appears after a change and disappears on Save.
- [ ] The same versions show up in SoundSource.

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
