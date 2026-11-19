//! The hearing-test tone output (Swift `ToneGenerator`): an IOProc straight on one output device
//! (no aggregate) rendering core's `ToneSynth` into the device's first two channels, silence on
//! the rest.
//!
//! Safety: the generator starts silent; every tone is limited to the per-frequency cap
//! (`TestCalibration::cap_dbfs`) here and again per sample in `ToneSynth`; a non-finite level or
//! a frequency outside the test range plays as silence; the IO block zeroes any non-finite
//! sample. The UI thread hands tones to the IO block through a lock-free command slot; the IO
//! block never allocates, locks or logs.

use std::cell::{Cell, UnsafeCell};
use std::ops::RangeInclusive;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::ptr::{self, NonNull};
use std::slice;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering, fence};

use audiocurve_core::hearing_test::TestCalibration;
use audiocurve_core::profile::Ear;
use audiocurve_core::tone::ToneSynth;
use block2::RcBlock;
use objc2_core_audio::{
    AudioDeviceCreateIOProcIDWithBlock, AudioDeviceDestroyIOProcID, AudioDeviceIOProcID,
    AudioDeviceStart, AudioDeviceStop, kAudioDevicePropertyDeviceIsAlive,
    kAudioDevicePropertyDeviceIsRunningSomewhere, kAudioDevicePropertyNominalSampleRate,
};
use objc2_core_audio_types::{AudioBuffer, AudioBufferList, AudioTimeStamp};

use super::hal::{self, Listener, ObjectId};
use super::route::{IoBlock, SAMPLE, buffers_mut, frame_count, mark_render_thread, zero};
use crate::platform::TestTone;

/// The test frequencies the cap table covers (Swift `ToneGenerator.frequencyRange`).
pub const FREQUENCY_RANGE: RangeInclusive<f64> = 125.0..=8000.0;

/// Below this the top test frequencies sit at or near Nyquist and play as silence (a headset in
/// call mode runs at 16 kHz); the test refuses to run (Swift `ToneGenerator.minimumSampleRate`).
pub const MINIMUM_SAMPLE_RATE: f64 = 22_050.0;

/// `Ok` if a device at this rate can play every test tone.
pub fn check_rate(rate: f64) -> Result<(), String> {
    if rate.is_finite() && rate >= MINIMUM_SAMPLE_RATE {
        Ok(())
    } else {
        Err(format!(
            "the output runs at {rate} Hz, below the {MINIMUM_SAMPLE_RATE} Hz the test needs"
        ))
    }
}

/// The tone actually played for a request: a non-finite level is silent, a frequency outside
/// `FREQUENCY_RANGE` plays as a silent 1 kHz tone, and the level never exceeds the cap.
fn sanitize(frequency_hz: f64, level_dbfs: Option<f64>) -> (f64, Option<f64>) {
    let mut level = level_dbfs;
    if level.is_some_and(|l| !l.is_finite()) {
        eprintln!("AudioCurve: non-finite tone level, playing silence");
        level = None;
    }
    let mut frequency = frequency_hz;
    if !frequency.is_finite() || !FREQUENCY_RANGE.contains(&frequency) {
        eprintln!("AudioCurve: tone frequency {frequency_hz} out of range, playing silence");
        frequency = 1000.0;
        level = None;
    }
    // First guard: never ask the synth for more than the cap (the sample clamp is the second).
    (
        frequency,
        level.map(|l| l.min(TestCalibration::cap_dbfs(frequency))),
    )
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Command {
    Tone {
        ear: Ear,
        frequency_hz: f64,
        level_dbfs: Option<f64>,
    },
    Silence,
}

const KIND_SILENCE: u8 = 0;
const KIND_LEFT: u8 = 1;
const KIND_RIGHT: u8 = 2;

/// The single-producer handoff from the UI thread to the IO block: a sequence lock over fixed
/// fields. The sequence is odd while a command is being written; each command's generation is
/// `sequence / 2`.
struct Slot {
    sequence: AtomicU64,
    kind: AtomicU8,
    frequency: AtomicU64,
    /// `f64` bits; NaN is a silent (`None`) level.
    level: AtomicU64,
    /// Generation of the latest command that finished rendering.
    finished: AtomicU64,
}

impl Slot {
    fn new() -> Slot {
        Slot {
            sequence: AtomicU64::new(0),
            kind: AtomicU8::new(KIND_SILENCE),
            frequency: AtomicU64::new(0),
            level: AtomicU64::new(f64::NAN.to_bits()),
            finished: AtomicU64::new(0),
        }
    }

    /// UI thread only (the one producer).
    fn post(&self, command: Command) {
        let s = self.sequence.load(Ordering::Relaxed);
        self.sequence.store(s + 1, Ordering::Relaxed);
        fence(Ordering::Release);
        let (kind, frequency, level) = match command {
            Command::Tone {
                ear,
                frequency_hz,
                level_dbfs,
            } => (
                if ear == Ear::Left {
                    KIND_LEFT
                } else {
                    KIND_RIGHT
                },
                frequency_hz,
                level_dbfs.unwrap_or(f64::NAN),
            ),
            Command::Silence => (KIND_SILENCE, 0.0, f64::NAN),
        };
        self.kind.store(kind, Ordering::Relaxed);
        self.frequency.store(frequency.to_bits(), Ordering::Relaxed);
        self.level.store(level.to_bits(), Ordering::Relaxed);
        self.sequence.store(s + 2, Ordering::Release);
    }

    /// IO thread. The newest command if it is complete and newer than `seen` (a sequence), with
    /// its sequence. A command caught mid-write is picked up on the next cycle.
    fn take(&self, seen: u64) -> Option<(u64, Command)> {
        let s1 = self.sequence.load(Ordering::Acquire);
        if s1 % 2 == 1 || s1 == seen {
            return None;
        }
        let kind = self.kind.load(Ordering::Relaxed);
        let frequency = f64::from_bits(self.frequency.load(Ordering::Relaxed));
        let level = f64::from_bits(self.level.load(Ordering::Relaxed));
        fence(Ordering::Acquire);
        if self.sequence.load(Ordering::Relaxed) != s1 {
            return None;
        }
        let command = match kind {
            KIND_LEFT | KIND_RIGHT => Command::Tone {
                ear: if kind == KIND_LEFT {
                    Ear::Left
                } else {
                    Ear::Right
                },
                frequency_hz: frequency,
                level_dbfs: (!level.is_nan()).then_some(level),
            },
            _ => Command::Silence,
        };
        Some((s1, command))
    }

    /// Generation of the latest command posted.
    fn requested(&self) -> u64 {
        self.sequence.load(Ordering::Acquire) / 2
    }

    /// A posted tone (or silent catch trial) has not finished rendering yet.
    fn is_sounding(&self) -> bool {
        self.requested() != self.finished.load(Ordering::Acquire)
    }
}

/// Frames rendered per synth call; the scratch buffers live on the IO thread's stack.
const CHUNK: usize = 256;

/// The IO block's state. Only the IOProc's one thread touches `synth` and the cells.
struct ToneRender {
    slot: Arc<Slot>,
    sample_rate: f64,
    synth: UnsafeCell<ToneSynth>,
    /// Sequence of the last command applied.
    seen: Cell<u64>,
}

impl ToneRender {
    fn new(slot: Arc<Slot>, sample_rate: f64) -> ToneRender {
        ToneRender {
            slot,
            sample_rate,
            synth: UnsafeCell::new(ToneSynth::new(sample_rate)),
            seen: Cell::new(0),
        }
    }

    /// The IO block's body. Nothing may unwind into Core Audio.
    ///
    /// # Safety
    /// As `render`.
    unsafe fn io(&self, output: NonNull<AudioBufferList>) {
        // The panic hook then lets a caught synth panic through instead of aborting.
        mark_render_thread();
        // SAFETY: forwarded caller contract.
        let rendered = catch_unwind(AssertUnwindSafe(|| unsafe {
            self.render(buffers_mut(output))
        }));
        if rendered.is_err() {
            // SAFETY: as above.
            zero(unsafe { buffers_mut(output) });
        }
    }

    /// Applies the newest command, then renders the synth into the first two channels.
    ///
    /// # Safety
    /// Core Audio's output buffers, from one thread at a time; each buffer's `mData` holds
    /// `mDataByteSize` writable bytes of `f32`.
    unsafe fn render(&self, out: &mut [AudioBuffer]) {
        // SAFETY: only the IO thread touches the synth.
        let synth = unsafe { &mut *self.synth.get() };
        if let Some((sequence, command)) = self.slot.take(self.seen.get()) {
            self.seen.set(sequence);
            match command {
                Command::Tone {
                    ear,
                    frequency_hz,
                    level_dbfs,
                } => synth.start(ear, frequency_hz, level_dbfs),
                Command::Silence => *synth = ToneSynth::new(self.sample_rate),
            }
        }
        zero(out);
        let frames = frame_count(out);
        let (mut l, mut r) = ([0f32; CHUNK], [0f32; CHUNK]);
        let mut offset = 0;
        while offset < frames {
            let n = CHUNK.min(frames - offset);
            let (l, r) = (&mut l[..n], &mut r[..n]);
            synth.render(l, r);
            // Last guard: never pass on a non-finite sample.
            for s in l.iter_mut().chain(r.iter_mut()) {
                if !s.is_finite() {
                    *s = 0.0;
                }
            }
            // SAFETY: forwarded caller contract.
            unsafe { write_channels(out, offset, l, r) };
            offset += n;
        }
        let generation = self.seen.get() / 2;
        if synth.is_finished() && self.slot.finished.load(Ordering::Relaxed) != generation {
            self.slot.finished.store(generation, Ordering::Release);
        }
    }
}

/// Writes `l` and `r` as frames `offset..` of the device's channels 0 and 1, counted across the
/// buffers (one interleaved buffer or several). Other channels are left as they are (zeroed).
///
/// # Safety
/// Each buffer's `mData` must hold `mDataByteSize` writable bytes of `f32`.
unsafe fn write_channels(out: &mut [AudioBuffer], offset: usize, l: &[f32], r: &[f32]) {
    let mut first_channel = 0;
    for buf in out.iter_mut() {
        let ch = buf.mNumberChannels.max(1) as usize;
        let base = first_channel;
        first_channel += ch;
        if buf.mData.is_null() || base >= 2 {
            continue;
        }
        // SAFETY: the buffer holds `mDataByteSize` bytes.
        let d = unsafe {
            slice::from_raw_parts_mut(buf.mData.cast::<f32>(), buf.mDataByteSize as usize / SAMPLE)
        };
        let frames = (d.len() / ch).saturating_sub(offset).min(l.len());
        for c in 0..ch.min(2 - base) {
            let src = if base + c == 0 { l } else { r };
            for i in 0..frames {
                d[(offset + i) * ch + c] = src[i];
            }
        }
    }
}

/// A started generator. Dropping it stops IO, destroys the IOProc and removes the listeners.
struct Running {
    device: ObjectId,
    proc_id: AudioDeviceIOProcID,
    block: Option<IoBlock>,
    slot: Arc<Slot>,
    /// Cleared when the device stops, dies or changes rate under the generator.
    device_ok: Arc<AtomicBool>,
    listeners: Vec<Listener>,
}

impl Drop for Running {
    fn drop(&mut self) {
        if self.proc_id.is_some() {
            // SAFETY: the IOProc created on this device.
            unsafe {
                AudioDeviceStop(self.device, self.proc_id);
                AudioDeviceDestroyIOProcID(self.device, self.proc_id);
            }
            self.proc_id = None;
        }
        self.block = None;
        self.listeners.clear();
    }
}

/// Is the device still alive, running, and at the rate the synth was built for?
fn device_still_ok(device: ObjectId, rate: f64) -> bool {
    // SAFETY: both properties are `UInt32`.
    let flag = |selector| unsafe { hal::get(device, selector, 0u32) }.is_ok_and(|v| v != 0);
    flag(kAudioDevicePropertyDeviceIsAlive)
        && flag(kAudioDevicePropertyDeviceIsRunningSomewhere)
        && hal::nominal_sample_rate(device) == Some(rate)
}

/// The real `TestTone`.
#[derive(Default)]
pub struct MacTestTone {
    running: Option<Running>,
}

impl MacTestTone {
    pub fn new() -> MacTestTone {
        MacTestTone::default()
    }

    /// A tone (or silent catch trial) posted by `play` has not finished rendering yet.
    pub fn is_tone_sounding(&self) -> bool {
        self.running.as_ref().is_some_and(|r| r.slot.is_sounding())
    }

    fn start_on(uid: &str) -> Result<Running, String> {
        let device = hal::device_by_uid(uid).ok_or_else(|| format!("output {uid} not found"))?;
        let rate = hal::nominal_sample_rate(device)
            .ok_or_else(|| format!("the sample rate of {uid} cannot be read"))?;
        check_rate(rate)?;
        let slot = Arc::new(Slot::new());
        let mut running = Running {
            device,
            proc_id: None,
            block: None,
            slot: slot.clone(),
            device_ok: Arc::new(AtomicBool::new(true)),
            listeners: Vec::new(),
        };
        let render = ToneRender::new(slot, rate);
        let block: IoBlock = RcBlock::new(
            move |_now: NonNull<AudioTimeStamp>,
                  _input: NonNull<AudioBufferList>,
                  _input_time: NonNull<AudioTimeStamp>,
                  output: NonNull<AudioBufferList>,
                  _output_time: NonNull<AudioTimeStamp>| {
                // SAFETY: Core Audio's output buffer list, on the IOProc's one thread.
                unsafe { render.io(output) }
            },
        );
        let mut proc_id: AudioDeviceIOProcID = None;
        // SAFETY: the block stays alive in `running.block` until the IOProc is destroyed.
        let status = unsafe {
            AudioDeviceCreateIOProcIDWithBlock(
                NonNull::from(&mut proc_id),
                device,
                None,
                ptr::from_ref(&*block).cast_mut(),
            )
        };
        running.block = Some(block);
        hal::check(status, "AudioDeviceCreateIOProcIDWithBlock").map_err(|e| e.to_string())?;
        running.proc_id = proc_id;
        // Every early return from here drops `running`, which stops and destroys the IOProc.
        // SAFETY: the IOProc just created on this device.
        let status = unsafe { AudioDeviceStart(device, proc_id) };
        hal::check(status, "AudioDeviceStart").map_err(|e| e.to_string())?;

        let watched = [
            kAudioDevicePropertyDeviceIsAlive,
            kAudioDevicePropertyDeviceIsRunningSomewhere,
            kAudioDevicePropertyNominalSampleRate,
        ];
        running.listeners = watched
            .into_iter()
            .map(|selector| {
                let ok = running.device_ok.clone();
                hal::listen(device, selector, move || {
                    if !device_still_ok(device, rate) {
                        ok.store(false, Ordering::Relaxed);
                    }
                })
            })
            .collect();
        Ok(running)
    }
}

impl TestTone for MacTestTone {
    /// Starts a silent generator on the output with this UID, replacing any running one.
    fn start(&mut self, uid: &str) -> Result<(), String> {
        self.stop();
        self.running = Some(Self::start_on(uid)?);
        Ok(())
    }

    /// Plays one pulsed tone (`ToneSynth::DURATION`); `None` is a silent catch trial. A tone
    /// already playing is replaced. Ignored while the generator is not running.
    fn play(&mut self, ear: Ear, frequency_hz: f64, level_dbfs: Option<f64>) {
        let Some(running) = &self.running else {
            eprintln!("AudioCurve: tone requested while the generator is not running");
            return;
        };
        let (frequency_hz, level_dbfs) = sanitize(frequency_hz, level_dbfs);
        running.slot.post(Command::Tone {
            ear,
            frequency_hz,
            level_dbfs,
        });
    }

    fn is_playing(&self) -> bool {
        self.running
            .as_ref()
            .is_some_and(|r| r.device_ok.load(Ordering::Relaxed))
    }

    /// Silences any tone, then stops and destroys the IOProc. `start` may be called again.
    fn stop(&mut self) {
        if let Some(running) = self.running.take() {
            running.slot.post(Command::Silence);
            drop(running);
        }
    }

    fn sample_rate(&self, uid: &str) -> f64 {
        hal::device_by_uid(uid)
            .and_then(hal::nominal_sample_rate)
            .unwrap_or(0.0)
    }
}

impl Drop for MacTestTone {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: f64 = 48_000.0;

    fn buffer(data: &mut [f32], channels: u32) -> AudioBuffer {
        AudioBuffer {
            mNumberChannels: channels,
            mDataByteSize: (data.len() * SAMPLE) as u32,
            mData: data.as_mut_ptr().cast(),
        }
    }

    /// One IO cycle of `frames` frames into a single interleaved buffer prefilled with 1.0.
    fn cycle(r: &ToneRender, frames: usize, channels: usize) -> Vec<f32> {
        let mut d = vec![1.0f32; frames * channels];
        let mut out = [buffer(&mut d, channels as u32)];
        // SAFETY: one valid interleaved buffer.
        unsafe { r.render(&mut out) };
        d
    }

    fn tone(ear: Ear, level: Option<f64>) -> Command {
        Command::Tone {
            ear,
            frequency_hz: 1000.0,
            level_dbfs: level,
        }
    }

    #[test]
    fn the_slot_hands_over_each_command_once() {
        let slot = Slot::new();
        assert_eq!(slot.take(0), None);
        slot.post(tone(Ear::Right, Some(-20.0)));
        let (seq, c) = slot.take(0).unwrap();
        assert_eq!(c, tone(Ear::Right, Some(-20.0)));
        assert_eq!(slot.take(seq), None, "already applied");
        slot.post(tone(Ear::Left, None));
        slot.post(Command::Silence);
        let (seq2, c) = slot.take(seq).unwrap();
        assert_eq!(c, Command::Silence, "only the newest");
        assert_eq!(seq2 / 2, slot.requested());
    }

    #[test]
    fn a_command_caught_mid_write_is_not_taken() {
        let slot = Slot::new();
        slot.post(tone(Ear::Left, Some(-30.0)));
        slot.sequence.fetch_add(1, Ordering::Relaxed);
        assert_eq!(slot.take(0), None);
    }

    #[test]
    fn sanitize_caps_and_silences() {
        assert_eq!(sanitize(1000.0, Some(-20.0)), (1000.0, Some(-20.0)));
        assert_eq!(sanitize(1000.0, Some(0.0)), (1000.0, Some(-3.0)));
        assert_eq!(sanitize(250.0, Some(5.0)), (250.0, Some(-1.0)));
        assert_eq!(sanitize(4000.0, Some(f64::NAN)), (4000.0, None));
        assert_eq!(sanitize(124.9, Some(-20.0)), (1000.0, None));
        assert_eq!(sanitize(8000.1, Some(-20.0)), (1000.0, None));
        assert_eq!(sanitize(f64::NAN, Some(-20.0)), (1000.0, None));
        assert_eq!(sanitize(125.0, None), (125.0, None));
        assert_eq!(sanitize(8000.0, Some(-40.0)), (8000.0, Some(-40.0)));
    }

    #[test]
    fn a_tone_plays_on_its_ear_and_silences_the_other_channels() {
        let slot = Arc::new(Slot::new());
        let r = ToneRender::new(slot.clone(), RATE);
        slot.post(tone(Ear::Right, Some(-20.0)));
        // 600 frames: more than one chunk, and into the first pulse.
        let d = cycle(&r, 600, 4);
        let frame = |i: usize| &d[i * 4..i * 4 + 4];
        assert!((0..600).all(|i| frame(i)[0] == 0.0 && frame(i)[2] == 0.0 && frame(i)[3] == 0.0));
        let peak = (0..600).map(|i| frame(i)[1].abs()).fold(0.0, f32::max);
        assert!(peak > 0.0 && peak <= 0.1 + 1e-6, "peak {peak}");
    }

    #[test]
    fn separate_mono_buffers_get_left_and_right() {
        let slot = Arc::new(Slot::new());
        let r = ToneRender::new(slot.clone(), RATE);
        slot.post(tone(Ear::Left, Some(-20.0)));
        let (mut a, mut b, mut c) = (vec![1.0f32; 600], vec![1.0f32; 600], vec![1.0f32; 600]);
        let mut out = [buffer(&mut a, 1), buffer(&mut b, 1), buffer(&mut c, 1)];
        // SAFETY: three valid mono buffers.
        unsafe { r.render(&mut out) };
        assert!(a.iter().any(|&x| x != 0.0));
        assert!(b.iter().chain(&c).all(|&x| x == 0.0));
    }

    #[test]
    fn a_silent_catch_trial_is_silent_and_finishes_after_its_duration() {
        let slot = Arc::new(Slot::new());
        let r = ToneRender::new(slot.clone(), RATE);
        assert!(!slot.is_sounding());
        slot.post(tone(Ear::Left, None));
        assert!(slot.is_sounding());
        let cycles = (ToneSynth::DURATION * RATE) as usize / 512;
        for _ in 0..cycles {
            assert!(cycle(&r, 512, 2).iter().all(|&x| x == 0.0));
        }
        assert!(slot.is_sounding(), "not yet the full duration");
        assert!(cycle(&r, 512, 2).iter().all(|&x| x == 0.0));
        assert!(!slot.is_sounding());
    }

    #[test]
    fn silence_cuts_a_tone_off() {
        let slot = Arc::new(Slot::new());
        let r = ToneRender::new(slot.clone(), RATE);
        slot.post(tone(Ear::Left, Some(-20.0)));
        assert!(cycle(&r, 512, 2).iter().any(|&x| x != 0.0));
        slot.post(Command::Silence);
        assert!(cycle(&r, 512, 2).iter().all(|&x| x == 0.0));
        assert!(!slot.is_sounding());
    }

    #[test]
    fn an_empty_or_null_output_is_harmless() {
        let r = ToneRender::new(Arc::new(Slot::new()), RATE);
        let mut out = [AudioBuffer {
            mNumberChannels: 2,
            mDataByteSize: 0,
            mData: ptr::null_mut(),
        }];
        // SAFETY: a null buffer, which `render` skips.
        unsafe { r.render(&mut out) };
        // SAFETY: no buffers at all.
        unsafe { r.render(&mut []) };
    }

    /// A synth panic is caught in the IO block: the panic hook must see a render thread, or it
    /// would abort mid-test with the volume at max.
    #[test]
    fn the_io_block_marks_its_thread_as_a_render_thread() {
        std::thread::spawn(|| {
            assert!(!super::super::route::is_render_thread());
            let r = ToneRender::new(Arc::new(Slot::new()), RATE);
            let mut d = vec![0f32; 2 * CHUNK];
            let mut out = AudioBufferList {
                mNumberBuffers: 1,
                mBuffers: [buffer(&mut d, 2)],
            };
            // SAFETY: one valid interleaved buffer.
            unsafe { r.io(NonNull::from(&mut out)) };
            assert!(super::super::route::is_render_thread());
        })
        .join()
        .unwrap();
    }
}
