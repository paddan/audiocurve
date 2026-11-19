//! The hearing test session (Swift `HearingTestModel`): drives core's `HearingTest`, plays its
//! tones through `TestTone` and owns the device volume while the test runs.
//!
//! Safety order before the first tone: the route is paused (`AppModel::testing`), the generator
//! starts silent, the original volume is saved as the pending snapshot, then raised to max. The
//! volume is raised only while the test is running: whenever it pauses or ends — pause, finish,
//! cancel, stop, quit — the tones go off first, then the volume is restored and the pending
//! snapshot cleared. A restore that fails (device gone) keeps the snapshot, so it is restored when
//! the device is back (also after a relaunch); raising again adopts it rather than re-reading.
//!
//! The Swift run loop is a state machine here: `tick(now)` runs what is due and
//! `next_deadline()` says when to call it next. Per presentation: the gap, the tone
//! (`ToneSynth::DURATION`), then the response window. A press ends the window early; no press is
//! "not heard". The environment is checked every `CHECK_INTERVAL`.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use audiocurve_core::hearing_test::{
    HearingTest, HearingTestRecord, HearingTestResult, HearingTestStep, Presentation,
    TestCalibration, Transducer,
};
use audiocurve_core::outputs::OutputDevice;
use audiocurve_core::profile::{Ear, HearingProfile};
use audiocurve_core::store::ProfileStore;
use audiocurve_core::tone::ToneSynth;
use audiocurve_core::volume::VolumeSnapshot;
use audiocurve_dsp::BAND_COUNT;
use uuid::Uuid;

use crate::i18n;
use crate::platform::{Activity, TestTone, Volume};

/// Time to answer after the tone ends.
const RESPONSE_WINDOW: Duration = Duration::from_millis(1500);
const CHECK_INTERVAL: Duration = Duration::from_secs(1);
/// Below this the generator cannot render the highest test tones (Swift
/// `ToneGenerator.minimumSampleRate`).
const MIN_SAMPLE_RATE: f64 = 44_100.0;

#[derive(Clone, Debug, PartialEq)]
pub enum Phase {
    Intro,
    Running(Presentation),
    Paused(PauseReason),
    Done(HearingTestResult),
    SaveFailed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PauseReason {
    User,
    OtherSound,
}

/// Why a running test stopped; shown on the intro screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StopReason {
    OutputChanged,
    OtherHost,
    WindowClosed,
    GeneratorStopped,
    GeneratorFailed,
    VolumeNotSet,
    LowSampleRate,
}

/// Where a running presentation is, and when that part ends.
#[derive(Clone, Copy, Debug)]
enum Step {
    Gap(Instant),
    Tone(Instant),
    Window(Instant),
}

impl Step {
    fn due(self) -> Instant {
        match self {
            Step::Gap(t) | Step::Tone(t) | Step::Window(t) => t,
        }
    }
}

/// The parts of the app a session works with, lent by `AppModel` for one call.
pub(crate) struct Env<'a> {
    pub tone: &'a mut dyn TestTone,
    pub volume: &'a mut dyn Volume,
    pub activity: &'a dyn Activity,
    pub store: &'a ProfileStore,
    /// `AppModel::testing`: the route is paused while it is set.
    pub testing: &'a mut bool,
}

/// What `AppModel::hearing_save` writes.
pub(crate) struct MeasuredSave {
    pub id: Uuid,
    pub name: String,
    pub profile: HearingProfile,
    pub record: HearingTestRecord,
    pub device: OutputDevice,
}

type Random = Rc<RefCell<Box<dyn FnMut() -> f64>>>;

pub struct HearingSession {
    phase: Phase,
    stop_reason: Option<StopReason>,
    /// The result, kept through a failed save.
    result: Option<HearingTestResult>,
    /// The output the test runs on, fixed at start.
    device: Option<OutputDevice>,
    test_date: SystemTime,
    /// The date every start uses instead of today's (screenshots); None in the app.
    fixed_date: Option<SystemTime>,
    /// The user's transducer choice; None follows `TestCalibration::default_transducer`.
    transducer_choice: Option<Transducer>,
    // Checks for the current output, refreshed every second on the intro and while paused.
    others_playing: bool,
    muted: bool,
    volume_settable: bool,
    /// The output runs below `MIN_SAMPLE_RATE`: the test cannot run.
    low_sample_rate: bool,
    /// A test ended on an output whose volume AudioCurve cannot set: the user raised it by hand.
    remind_volume_down: bool,
    /// The app's current output and other hosts, as of the last call from `AppModel`.
    output: Option<OutputDevice>,
    other_hosts: Vec<String>,
    test: Option<HearingTest>,
    calibration: Option<TestCalibration>,
    /// The generator exists (started at least once and not ended).
    generator: bool,
    /// The volume to give back; None when nothing is raised.
    snapshot: Option<VolumeSnapshot>,
    /// This test runs on an output without a settable volume (the user set it to max).
    manual_volume: bool,
    step: Option<Step>,
    heard: bool,
    check_at: Instant,
    /// Reused by a retry so the result is never saved twice.
    save_id: Option<Uuid>,
    random: Random,
}

impl HearingSession {
    /// A session on its intro. `fixed_date` replaces today's date as the test date (for
    /// screenshots that look the same on every day); the app passes None.
    pub(crate) fn new(
        random: Box<dyn FnMut() -> f64>,
        fixed_date: Option<SystemTime>,
        now: Instant,
    ) -> HearingSession {
        HearingSession {
            phase: Phase::Intro,
            stop_reason: None,
            result: None,
            device: None,
            test_date: fixed_date.unwrap_or_else(SystemTime::now),
            fixed_date,
            transducer_choice: None,
            others_playing: false,
            muted: false,
            volume_settable: true,
            low_sample_rate: false,
            remind_volume_down: false,
            output: None,
            other_hosts: Vec::new(),
            test: None,
            calibration: None,
            generator: false,
            snapshot: None,
            manual_volume: false,
            step: None,
            heard: false,
            check_at: now + CHECK_INTERVAL,
            save_id: None,
            random: Rc::new(RefCell::new(random)),
        }
    }

    // MARK: State

    pub fn phase(&self) -> &Phase {
        &self.phase
    }

    pub fn stop_reason(&self) -> Option<StopReason> {
        self.stop_reason
    }

    pub fn result(&self) -> Option<&HearingTestResult> {
        self.result.as_ref()
    }

    /// The output the test runs on (set at start).
    pub fn device(&self) -> Option<&OutputDevice> {
        self.device.as_ref()
    }

    pub fn test_date(&self) -> SystemTime {
        self.test_date
    }

    /// Replaces the date the result is saved under (set when the test starts); for tests and
    /// screenshots.
    pub fn set_test_date(&mut self, date: SystemTime) {
        self.test_date = date;
    }

    pub fn transducer_choice(&self) -> Option<Transducer> {
        self.transducer_choice
    }

    pub fn set_transducer_choice(&mut self, t: Option<Transducer>) {
        self.transducer_choice = t;
    }

    pub fn others_playing(&self) -> bool {
        self.others_playing
    }

    pub fn muted(&self) -> bool {
        self.muted
    }

    pub fn volume_settable(&self) -> bool {
        self.volume_settable
    }

    pub fn low_sample_rate(&self) -> bool {
        self.low_sample_rate
    }

    pub fn remind_volume_down(&self) -> bool {
        self.remind_volume_down
    }

    pub fn transducer(&self) -> Transducer {
        self.transducer_choice.unwrap_or_else(|| {
            TestCalibration::default_transducer(self.output.as_ref().map_or("", |o| &o.name))
        })
    }

    /// The bundle ID of another host processing the sound, if any: the test cannot start then.
    pub fn blocking_host(&self) -> Option<&str> {
        self.other_hosts.first().map(String::as_str)
    }

    pub fn can_start(&self) -> bool {
        self.phase == Phase::Intro
            && self.output.is_some()
            && self.other_hosts.is_empty()
            && !self.muted
            && !self.others_playing
            && !self.low_sample_rate
    }

    /// Paused, and nothing would make the raised volume unsafe or the tones inaudible: the same
    /// checks as Start.
    pub fn can_resume(&self) -> bool {
        matches!(self.phase, Phase::Paused(_))
            && !self.muted
            && !self.others_playing
            && !self.low_sample_rate
    }

    pub fn is_active(&self) -> bool {
        matches!(self.phase, Phase::Running(_) | Phase::Paused(_))
    }

    /// The device whose volume this test holds raised (its pending snapshot must not be restored
    /// behind the test's back), or None.
    pub fn held_volume_uid(&self) -> Option<&str> {
        self.snapshot.as_ref().map(|s| s.device_uid.as_str())
    }

    /// "<device> – <date>", the suggested profile name.
    pub fn default_name(&self, formatted_date: &str) -> String {
        let name = self
            .device
            .as_ref()
            .or(self.output.as_ref())
            .map_or("", |d| d.name.as_str());
        HearingTestRecord::default_name(name, formatted_date)
    }

    /// [`Self::default_name`] with the test date in the current language.
    pub fn localized_default_name(&self) -> String {
        self.default_name(&i18n::formatted_date(self.test_date, i18n::lang()))
    }

    pub(crate) fn next_deadline(&self) -> Option<Instant> {
        let step = match self.phase {
            Phase::Running(_) => self.step.map(Step::due),
            _ => None,
        };
        Some(step.map_or(self.check_at, |s| s.min(self.check_at)))
    }

    // MARK: Driven by AppModel

    /// Takes in the app's current output and other hosts.
    pub(crate) fn observe(&mut self, output: Option<&OutputDevice>, other_hosts: &[String]) {
        if self.output.as_ref() != output {
            self.output = output.cloned();
        }
        if self.other_hosts != other_hosts {
            self.other_hosts = other_hosts.to_vec();
        }
    }

    /// Runs the environment check and the presentation steps that are due.
    pub(crate) fn tick(&mut self, env: &mut Env, now: Instant) {
        if now >= self.check_at {
            self.check_at = now + CHECK_INTERVAL;
            self.check_environment(env);
        }
        while let (Phase::Running(p), Some(step)) = (&self.phase, self.step) {
            if step.due() > now {
                break;
            }
            let p = p.clone();
            match step {
                Step::Gap(_) => {
                    if !self.generator || !env.tone.is_playing() {
                        self.stop(env, Some(StopReason::GeneratorStopped));
                        return;
                    }
                    self.heard = false;
                    env.tone.play(p.ear, p.frequency_hz, p.level_dbfs);
                    let tone = Duration::from_secs_f64(ToneSynth::DURATION);
                    self.step = Some(Step::Tone(now + tone));
                }
                Step::Tone(end) => self.step = Some(Step::Window(end + RESPONSE_WINDOW)),
                Step::Window(_) => self.answer(env, now),
            }
        }
    }

    /// Stops the test when the output, the other hosts, the sample rate or the generator changed;
    /// pauses it when another app plays. On the intro screen and while paused, refreshes the checks.
    pub(crate) fn check_environment(&mut self, env: &mut Env) {
        match self.phase {
            Phase::Intro => self.refresh_checks(env),
            Phase::Running(_) | Phase::Paused(_) => {
                let Some(uid) = self.device.as_ref().map(|d| d.uid.clone()) else {
                    return;
                };
                if self.output.as_ref().map(|o| &o.uid) != Some(&uid) {
                    self.stop(env, Some(StopReason::OutputChanged));
                } else if !self.other_hosts.is_empty() {
                    self.stop(env, Some(StopReason::OtherHost));
                } else if env.tone.sample_rate(&uid) < MIN_SAMPLE_RATE {
                    self.stop(env, Some(StopReason::LowSampleRate));
                } else if matches!(self.phase, Phase::Running(_)) {
                    if !self.generator || !env.tone.is_playing() {
                        self.stop(env, Some(StopReason::GeneratorStopped));
                    } else if env.activity.others_playing() {
                        self.pause(env, PauseReason::OtherSound);
                    }
                } else {
                    self.refresh_checks(env);
                }
            }
            Phase::Done(_) | Phase::SaveFailed => {}
        }
    }

    /// The checks Start makes, read now. True when the test may start.
    pub(crate) fn may_start(&mut self, env: &mut Env) -> bool {
        if self.phase != Phase::Intro || self.output.is_none() || !self.other_hosts.is_empty() {
            return false;
        }
        self.refresh_checks(env);
        self.can_start()
    }

    /// Starts the test after `may_start`: route paused, generator started silent, volume saved
    /// and raised, then the first presentation.
    pub(crate) fn start(&mut self, env: &mut Env, now: Instant) {
        let Some(out) = self.output.clone() else {
            return;
        };
        self.stop_reason = None;
        self.remind_volume_down = false;
        self.manual_volume = false;
        *env.testing = true;
        self.device = Some(out.clone());
        self.generator = true;
        if let Err(e) = env.tone.start(&out.uid) {
            eprintln!("AudioCurve: hearing test generator failed: {e}");
            self.end_active(env);
            self.stop_reason = Some(StopReason::GeneratorFailed);
            return;
        }
        if !self.raise_volume(env) {
            self.end_active(env);
            self.stop_reason = Some(StopReason::VolumeNotSet);
            return;
        }
        let cal = TestCalibration::nominal(self.transducer());
        self.calibration = Some(cal);
        let random = self.random.clone();
        self.test = Some(HearingTest::new(
            cal,
            Box::new(move || (random.borrow_mut())()),
        ));
        self.test_date = self.fixed_date.unwrap_or_else(SystemTime::now);
        self.advance(env, now);
    }

    /// "I hear it". Counts only while a tone or its response window is open.
    pub(crate) fn respond(&mut self, env: &mut Env, now: Instant) {
        if matches!(self.phase, Phase::Running(_))
            && matches!(self.step, Some(Step::Tone(_) | Step::Window(_)))
        {
            self.heard = true;
            self.answer(env, now);
        }
    }

    /// Tones off, then the volume back at once (it is raised only while the test runs); resume
    /// raises it again. The unanswered presentation is played again on resume.
    pub(crate) fn pause(&mut self, env: &mut Env, reason: PauseReason) {
        if !matches!(self.phase, Phase::Running(_)) {
            return;
        }
        self.step = None;
        env.tone.stop();
        self.restore_volume(env);
        self.phase = Phase::Paused(reason);
        self.refresh_checks(env);
    }

    /// Starts the generator and raises the volume again, unless another app plays, the output is
    /// muted or runs at a low sample rate (checked now, as Start does).
    pub(crate) fn resume(&mut self, env: &mut Env, now: Instant) {
        if !matches!(self.phase, Phase::Paused(_)) || !self.generator {
            return;
        }
        self.refresh_checks(env);
        if !self.can_resume() {
            return;
        }
        let Some(uid) = self.device.as_ref().map(|d| d.uid.clone()) else {
            return;
        };
        if let Err(e) = env.tone.start(&uid) {
            eprintln!("AudioCurve: hearing test generator failed on resume: {e}");
            self.stop(env, Some(StopReason::GeneratorStopped));
            return;
        }
        if !self.raise_volume(env) {
            self.stop(env, Some(StopReason::VolumeNotSet));
            return;
        }
        self.advance(env, now);
    }

    /// Cancel: true when the session should close. On an output whose volume the user raised by
    /// hand, a running test goes back to the intro instead, which asks them to turn it down.
    pub(crate) fn cancel(&mut self, env: &mut Env) -> bool {
        if self.is_active() && self.manual_volume {
            self.stop(env, None);
            return false;
        }
        true
    }

    /// The window closed: a running test stops.
    pub(crate) fn window_closed(&mut self, env: &mut Env) {
        self.stop(env, Some(StopReason::WindowClosed));
    }

    /// Stops everything; for closing the session and for quitting.
    pub(crate) fn shut_down(&mut self, env: &mut Env) {
        self.end_active(env);
    }

    /// What to save, in `Done` or after a failed save; None otherwise.
    pub(crate) fn measured_save(&mut self, name: &str) -> Option<MeasuredSave> {
        if !matches!(self.phase, Phase::Done(_) | Phase::SaveFailed) {
            return None;
        }
        let (result, device, calibration) = (
            self.result.as_ref()?,
            self.device.clone()?,
            self.calibration?,
        );
        let id = *self.save_id.get_or_insert_with(Uuid::new_v4);
        let zeros = vec![0.0; BAND_COUNT];
        let profile = HearingProfile {
            left: result.audiogram(Ear::Left),
            right: result.audiogram(Ear::Right),
            left_trim_db: zeros.clone(),
            right_trim_db: zeros,
        };
        let record = HearingTestRecord {
            date: self.test_date,
            output: device.clone(),
            calibration,
            left: result.left.clone(),
            right: result.right.clone(),
            flags: result.flags.clone(),
            app_version: env!("CARGO_PKG_VERSION").into(),
        };
        let trimmed = name.trim();
        let name = if trimmed.is_empty() {
            self.localized_default_name()
        } else {
            trimmed.to_owned()
        };
        Some(MeasuredSave {
            id,
            name,
            profile,
            record,
            device,
        })
    }

    pub(crate) fn save_failed(&mut self) {
        self.phase = Phase::SaveFailed;
    }

    // MARK: Running

    /// The next presentation after the gap, or the result.
    fn advance(&mut self, env: &mut Env, now: Instant) {
        let Some(test) = self.test.as_mut() else {
            return;
        };
        match test.next() {
            HearingTestStep::Finished(r) => self.finish(env, r),
            HearingTestStep::Present(p) => {
                self.step = Some(Step::Gap(now + Duration::from_secs_f64(p.gap_before_s)));
                self.phase = Phase::Running(p);
            }
        }
    }

    /// Closes the response window with the answer so far and moves on.
    fn answer(&mut self, env: &mut Env, now: Instant) {
        self.step = None;
        if let Some(test) = self.test.as_mut() {
            test.respond(self.heard);
        }
        self.advance(env, now);
    }

    fn finish(&mut self, env: &mut Env, r: HearingTestResult) {
        self.end_active(env);
        self.test = None;
        self.result = Some(r.clone());
        self.phase = Phase::Done(r);
    }

    /// Ends a running or paused test and goes back to the intro with `reason` (None: cancelled).
    fn stop(&mut self, env: &mut Env, reason: Option<StopReason>) {
        if !self.is_active() {
            return;
        }
        self.end_active(env);
        self.test = None;
        self.phase = Phase::Intro;
        self.stop_reason = reason;
        self.refresh_checks(env);
    }

    /// Tones off, volume back, route resumed. Safe to call more than once.
    fn end_active(&mut self, env: &mut Env) {
        self.step = None;
        if self.generator {
            env.tone.stop();
            self.generator = false;
        }
        self.restore_volume(env);
        if self.manual_volume {
            self.remind_volume_down = true;
        }
        *env.testing = false;
    }

    // MARK: Volume

    /// Saves the volume and raises it to max, with the generator running silent. A device without
    /// a settable volume passes (the intro asks the user to set it to max). False if the generator
    /// is not running or the volume could not be raised (then it is restored).
    ///
    /// A snapshot still pending for the device (a restore that failed at pause, or an earlier test)
    /// is the original: it is adopted, never replaced by the volume read now, which may be max.
    fn raise_volume(&mut self, env: &mut Env) -> bool {
        let Some(uid) = self.device.as_ref().map(|d| d.uid.clone()) else {
            return false;
        };
        if !self.generator || !env.tone.is_playing() {
            return false;
        }
        if let Some(now) = env.volume.snapshot(&uid) {
            env.store.save_pending_volume(now);
        }
        let Some(s) = env.store.pending_volume(&uid) else {
            self.manual_volume = true;
            return true;
        };
        self.snapshot = Some(s);
        if env.volume.set_max(&uid) {
            return true;
        }
        self.restore_volume(env);
        false
    }

    /// Gives the volume back. What could not be restored stays pending and is retried
    /// (`AppModel::restore_pending_volumes`) until it is done or given up.
    fn restore_volume(&mut self, env: &mut Env) {
        let Some(s) = self.snapshot.take() else {
            return;
        };
        let outcomes = env.volume.restore(&s);
        env.store
            .update_pending_volume(&s.device_uid, s.remaining(&outcomes));
    }

    pub(crate) fn refresh_checks(&mut self, env: &mut Env) {
        let Some(uid) = self.output.as_ref().map(|o| o.uid.clone()) else {
            self.others_playing = false;
            self.muted = false;
            self.volume_settable = true;
            self.low_sample_rate = false;
            return;
        };
        self.others_playing = env.activity.others_playing();
        self.muted = env.volume.is_muted(&uid);
        self.volume_settable = env.volume.is_settable(&uid);
        self.low_sample_rate = env.tone.sample_rate(&uid) < MIN_SAMPLE_RATE;
    }
}

/// A xorshift source in [0, 1) seeded from the clock, for the test's gaps and catch trials.
pub fn clock_random() -> Box<dyn FnMut() -> f64> {
    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos() as u64);
    let mut x = seed | 1;
    Box::new(move || {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        (x >> 11) as f64 / (1u64 << 53) as f64
    })
}
