use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

use audiocurve_core::outputs::{ActiveOutputs, OutputDevice};
use audiocurve_core::route::{
    BuiltRoute, PermissionState, PluginLoadState, RouteBuildError, RouteBuilder, RouteController, RouteEvents,
    RouteFailure, RouteInputs, RouteTarget as T,
};
use uuid::Uuid;

type Log = Rc<RefCell<Vec<String>>>;

struct FakeRoute {
    id: Uuid,
    log: Log,
}

impl BuiltRoute for FakeRoute {
    fn id(&self) -> Uuid {
        self.id
    }
    fn tear_down(self: Box<Self>) {
        self.log.borrow_mut().extend(["stopIO", "destroyAggregate", "destroyTap"].map(String::from));
    }
}

struct FakeBuilder {
    log: Log,
    failures_left: Rc<Cell<u32>>,
}

impl RouteBuilder for FakeBuilder {
    fn build(&mut self, device_uid: &str) -> Result<Box<dyn BuiltRoute>, RouteBuildError> {
        self.log.borrow_mut().push(format!("build:{device_uid}"));
        if self.failures_left.get() > 0 {
            self.failures_left.set(self.failures_left.get() - 1);
            return Err(RouteBuildError("fake".into()));
        }
        Ok(Box::new(FakeRoute { id: Uuid::new_v4(), log: self.log.clone() }))
    }
}

struct Harness {
    c: RouteController,
    log: Log,
    failures_left: Rc<Cell<u32>>,
    clock: Instant,
    reloads: u32,
    seen: Vec<T>,
}

fn teardown() -> Vec<String> {
    ["stopIO", "destroyAggregate", "destroyTap"].map(String::from).to_vec()
}

fn inputs_for(output: Option<&str>, active: ActiveOutputs) -> RouteInputs {
    RouteInputs {
        enabled: true,
        permission: PermissionState::Granted,
        plugin: PluginLoadState::Loaded,
        output_uid: output.map(String::from),
        active_outputs: active,
        testing: false,
    }
}

fn inputs(output: &str) -> RouteInputs {
    inputs_for(Some(output), ActiveOutputs::default())
}

fn running(uid: &str) -> T {
    T::Running { device_uid: uid.into() }
}

impl Harness {
    fn new() -> Self {
        let log: Log = Rc::default();
        let failures_left = Rc::new(Cell::new(0));
        let c = RouteController::new(Box::new(FakeBuilder { log: log.clone(), failures_left: failures_left.clone() }));
        Harness { c, log, failures_left, clock: Instant::now(), reloads: 0, seen: vec![] }
    }

    /// A controller already running on AirPods, with the log cleared.
    fn running() -> Self {
        let mut h = Self::new();
        h.set(inputs("airpods"));
        h.fire();
        h.log.borrow_mut().clear();
        h
    }

    fn take(&mut self, e: RouteEvents) {
        self.seen.extend(e.state_changes);
        self.reloads += u32::from(e.reload_plugin);
    }

    fn set(&mut self, i: RouteInputs) {
        self.c.set_inputs(i, self.clock);
    }

    fn set_plugin(&mut self, p: PluginLoadState) {
        let mut i = self.c.inputs().clone();
        i.plugin = p;
        self.set(i);
    }

    /// Runs what is scheduled: advances the clock to the next deadline and ticks.
    fn fire(&mut self) {
        if let Some(d) = self.c.next_deadline() {
            self.clock = self.clock.max(d);
            let e = self.c.tick(self.clock);
            self.take(e);
        }
    }

    fn plugin_died(&mut self, route_id: Option<Uuid>) {
        let e = self.c.plugin_died(route_id, self.clock);
        self.take(e);
    }

    fn log(&self) -> Vec<String> {
        self.log.borrow().clone()
    }

    fn advance(&mut self, secs: u64) {
        self.clock += Duration::from_secs(secs);
    }
}

#[test]
fn running_builds_once_after_coalescing() {
    let mut h = Harness::new();
    let t0 = h.clock;
    h.set(inputs("airpods"));
    assert!(h.log().is_empty());
    assert_eq!(h.c.next_deadline(), Some(t0 + RouteController::COALESCE));
    h.fire();
    assert_eq!(h.log(), ["build:airpods"]);
    assert_eq!(h.c.state(), &running("airpods"));
}

#[test]
fn burst_of_changes_coalesces_into_one_build() {
    let mut h = Harness::new();
    h.set(inputs("a"));
    h.set(inputs("b"));
    h.set(inputs("airpods"));
    h.fire();
    assert_eq!(h.log(), ["build:airpods"]);
}

#[test]
fn device_loss_tears_down_before_rebuild() {
    let mut h = Harness::running();
    h.set(inputs("spk"));
    h.fire();
    assert_eq!(h.log(), [teardown(), vec!["build:spk".into()]].concat());
    assert_eq!(h.c.state(), &running("spk"));
}

#[test]
fn pausing_tears_down_and_does_not_build() {
    let mut h = Harness::running();
    let only_aids = ActiveOutputs { devices: vec![OutputDevice { uid: "aids".into(), name: "Hearing aids".into() }] };
    h.set(inputs_for(Some("airpods"), only_aids));
    h.fire();
    assert_eq!(h.log(), teardown());
    assert_eq!(h.c.state(), &T::Paused { device_uid: "airpods".into() });
}

#[test]
fn build_failure_retries_once_then_fails() {
    let mut h = Harness::new();
    h.failures_left.set(2);
    h.set(inputs("airpods"));
    h.fire();
    assert_eq!(h.c.next_deadline(), Some(h.clock + RouteController::RETRY_DELAY));
    h.fire();
    assert_eq!(h.log(), ["build:airpods", "build:airpods"]);
    assert_eq!(h.c.state(), &T::Failed(RouteFailure::Route));
    assert_eq!(h.c.next_deadline(), None);
}

#[test]
fn retry_succeeds() {
    let mut h = Harness::new();
    h.failures_left.set(1);
    h.set(inputs("airpods"));
    h.fire();
    h.fire();
    assert_eq!(h.c.state(), &running("airpods"));
}

#[test]
fn plugin_death_tears_down_immediately_and_reloads() {
    let mut h = Harness::running();
    h.plugin_died(None);
    assert_eq!(h.log(), teardown());
    assert_eq!(h.reloads, 1);
    assert_eq!(h.c.inputs().plugin, PluginLoadState::Loading);
    assert_eq!(h.c.state(), &T::Starting);
}

#[test]
fn second_plugin_death_within_ten_seconds_fails() {
    let mut h = Harness::running();
    h.plugin_died(None);
    h.set_plugin(PluginLoadState::Loaded);
    h.fire();
    h.advance(5);
    h.plugin_died(None);
    assert_eq!(h.c.state(), &T::Failed(RouteFailure::Plugin));
    assert_eq!(h.reloads, 1);
    assert_eq!(h.log(), [teardown(), vec!["build:airpods".into()], teardown()].concat());
}

#[test]
fn plugin_death_after_ten_seconds_reloads_again() {
    let mut h = Harness::running();
    h.plugin_died(None);
    h.set_plugin(PluginLoadState::Loaded);
    h.fire();
    h.advance(11);
    h.plugin_died(None);
    assert_eq!(h.reloads, 2);
}

#[test]
fn unchanged_target_does_not_rebuild() {
    let mut h = Harness::running();
    h.set(inputs("airpods"));
    h.fire();
    assert!(h.log().is_empty());
}

#[test]
fn invalidate_rebuilds_same_device() {
    let mut h = Harness::running();
    h.c.invalidate(h.clock);
    h.fire();
    assert_eq!(h.log(), [teardown(), vec!["build:airpods".into()]].concat());
}

#[test]
fn shut_down_tears_down() {
    let mut h = Harness::running();
    let e = h.c.shut_down();
    h.take(e);
    assert_eq!(h.log(), teardown());
    assert_eq!(h.c.state(), &T::Off);
    h.set(inputs("spk"));
    h.fire();
    assert_eq!(h.log(), teardown());
}

#[test]
fn state_changes_are_reported_once() {
    let mut h = Harness::new();
    h.set(inputs("airpods"));
    h.fire();
    h.set(inputs("airpods"));
    h.fire();
    assert_eq!(h.seen, [running("airpods")]);
}

#[test]
fn state_is_not_running_while_a_retry_is_pending() {
    let mut h = Harness::running();
    h.failures_left.set(1);
    h.set(inputs("spk"));
    h.fire();
    assert_eq!(h.c.state(), &T::Starting);
    h.fire();
    assert_eq!(h.c.state(), &running("spk"));
}

#[test]
fn failed_plugin_stays_failed_when_inputs_say_loaded() {
    let mut h = Harness::running();
    h.plugin_died(None);
    h.set_plugin(PluginLoadState::Loaded);
    h.fire();
    h.advance(5);
    h.plugin_died(None);
    h.log.borrow_mut().clear();
    // The app writes its own idea of the plugin state on the next device change.
    h.set(inputs("spk"));
    h.fire();
    assert_eq!(h.c.state(), &T::Failed(RouteFailure::Plugin));
    assert!(h.log().is_empty());
    let e = h.c.retry_plugin(h.clock);
    h.take(e);
    assert_eq!(h.reloads, 2);
    assert_eq!(h.c.state(), &T::Starting);
}

#[test]
fn late_failure_from_a_replaced_route_is_ignored() {
    let mut h = Harness::running();
    let old_route = h.c.route_id();
    h.set(inputs("spk"));
    h.fire();
    h.log.borrow_mut().clear();
    h.plugin_died(old_route);
    assert!(h.log().is_empty());
    assert_eq!(h.reloads, 0);
    assert_eq!(h.c.state(), &running("spk"));
}

#[test]
fn early_tick_does_nothing() {
    let mut h = Harness::new();
    let t0 = h.clock;
    h.set(inputs("airpods"));
    let deadline = h.c.next_deadline().unwrap();
    assert!(h.c.tick(t0 + Duration::from_millis(100)).state_changes.is_empty());
    assert!(h.log().is_empty());
    assert_eq!(h.c.next_deadline(), Some(deadline));
}
