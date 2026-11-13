# Spike notes: AudioCurve 2

Date: 2026-11-13. Debug build, plugin loaded with `.loadOutOfProcess`.

## Q1: app ↔ plugin channel across the process boundary

- `AUAudioUnit.messageChannel(for:)` on the host side returns an `AUMessageChannel_XPC` proxy, but `callAudioUnit` returns an empty dictionary: the plugin's reply does not come back. Not usable.
- `fullState` does cross the boundary both ways: a value the plugin adds in the getter reaches the app, and a dictionary the app sets reaches the plugin's setter.

**Decision:** the channel is `fullState`. The plugin's getter adds `PluginRequest.statusKey` with an encoded status (profiles, active profile ID, Active on, other hosts). The app sends a command by setting `fullState` to a dictionary that contains only `PluginRequest.commandKey`; the plugin's setter runs the command and returns without touching parameters or the profile. `PluginRequest`/`PluginReply` encode these values.

## Q2: capture permission

- The first `AudioDeviceStart` on the tap's aggregate device shows the system prompt ("System Audio Recording Only"); the start call blocked until it was answered. After Allow, audio flowed at once.
- Denial was not tested (the prompt was allowed). Expected from Apple's sample code: the tap is created and delivers silence.

**Decision:** permission is detected as denied when the input has no non-zero sample for 3 s while `kAudioDevicePropertyDeviceIsRunningSomewhere` is true for the output device. `AudioDeviceStart` must not run on the main thread, because it blocks while the prompt is up.

## Q3: buffer size

- AirPods Pro at 44.1 kHz: the aggregate device reported 512 frames, and the IOProc ran with 512 frames per call (about 86 calls/s). 512 frames at 44.1 kHz is 11.6 ms per buffer.

## Developer note

Only one copy of the plugin is registered at a time, and macOS kept the copy in `/Applications` (and an old DerivedData copy) over a Debug build. To run a Debug build against its own plugin: quit SoundSource, `pluginkit -r` the other copies and `lsregister -u` their apps, then `pluginkit -a` the Debug `.appex`. Afterwards restore `/Applications`, re-register it and `killall arkaudiod`.
