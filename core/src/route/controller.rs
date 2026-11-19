//! Owns the system-wide route and keeps it matching `route_target(inputs)` (Swift
//! `RouteController`). Every path that leaves `Running` tears the route down first, so the
//! system is never left muted.
//!
//! The Swift version schedules work with timers. This one is driven from outside: it records
//! deadlines, and the owner calls `tick(now)` when `next_deadline()` is due. All times come in
//! as arguments. It is driven from one thread and is not `Send`.

use std::time::{Duration, Instant};

use uuid::Uuid;

use super::decision::{PermissionState, PluginLoadState, RouteFailure, RouteInputs, RouteTarget, route_target};
use crate::outputs::ActiveOutputs;

/// One built route. `tear_down` does the platform's teardown in the right order (on macOS:
/// stop IO, destroy the aggregate device, destroy the tap, which unmutes the system).
pub trait BuiltRoute {
    /// Identifies this route in `RouteController::plugin_died`.
    fn id(&self) -> Uuid;
    fn tear_down(self: Box<Self>);
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RouteBuildError(pub String);

pub trait RouteBuilder {
    /// Creates the route for the output device and starts IO.
    fn build(&mut self, device_uid: &str) -> Result<Box<dyn BuiltRoute>, RouteBuildError>;
}

/// What a call changed: every state change in order, and whether the owner should reload the
/// plugin (engine).
#[derive(Debug, Default, PartialEq)]
pub struct RouteEvents {
    pub state_changes: Vec<RouteTarget>,
    pub reload_plugin: bool,
}

struct Retry {
    at: Instant,
    generation: u64,
    device_uid: String,
}

pub struct RouteController {
    builder: Box<dyn RouteBuilder>,
    inputs: RouteInputs,
    state: RouteTarget,
    route: Option<(String, Box<dyn BuiltRoute>)>,
    /// A coalesced apply is due at this time.
    apply_at: Option<Instant>,
    retry: Option<Retry>,
    force_rebuild: bool,
    last_death: Option<Instant>,
    is_shut_down: bool,
    /// Set after a second plugin death within `DEATH_WINDOW`: inputs cannot clear it, only
    /// `retry_plugin` can, so a device change does not start a reload/mute cycle.
    plugin_failed: bool,
    /// Bumped on every apply, so a retry scheduled for an older apply does nothing.
    generation: u64,
    events: RouteEvents,
}

impl RouteController {
    pub const COALESCE: Duration = Duration::from_millis(200);
    pub const RETRY_DELAY: Duration = Duration::from_millis(500);
    pub const DEATH_WINDOW: Duration = Duration::from_secs(10);

    pub fn new(builder: Box<dyn RouteBuilder>) -> Self {
        Self {
            builder,
            inputs: RouteInputs {
                enabled: false,
                permission: PermissionState::Unknown,
                plugin: PluginLoadState::Loading,
                output_uid: None,
                active_outputs: ActiveOutputs::default(),
                testing: false,
            },
            state: RouteTarget::Off,
            route: None,
            apply_at: None,
            retry: None,
            force_rebuild: false,
            last_death: None,
            is_shut_down: false,
            plugin_failed: false,
            generation: 0,
            events: RouteEvents::default(),
        }
    }

    pub fn inputs(&self) -> &RouteInputs {
        &self.inputs
    }

    pub fn state(&self) -> &RouteTarget {
        &self.state
    }

    /// The current route's ID, if a route is built.
    pub fn route_id(&self) -> Option<Uuid> {
        self.route.as_ref().map(|(_, r)| r.id())
    }

    /// A change schedules one apply after `COALESCE`; several changes in that time give one apply.
    pub fn set_inputs(&mut self, inputs: RouteInputs, now: Instant) {
        if inputs != self.inputs {
            self.inputs = inputs;
            self.schedule_apply(now);
        }
    }

    /// Forces a teardown and rebuild after `COALESCE`, even if the target is unchanged (the
    /// output device's sample rate changed).
    pub fn invalidate(&mut self, now: Instant) {
        self.force_rebuild = true;
        self.schedule_apply(now);
    }

    /// When `tick` next has work to do.
    pub fn next_deadline(&self) -> Option<Instant> {
        match (self.apply_at, self.retry.as_ref().map(|r| r.at)) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }

    /// Runs whatever is due at `now`: a coalesced apply, a build retry. Nothing before a deadline.
    pub fn tick(&mut self, now: Instant) -> RouteEvents {
        loop {
            let apply_due = self.apply_at.filter(|&t| t <= now);
            let retry_due = self.retry.as_ref().map(|r| r.at).filter(|&t| t <= now);
            match (apply_due, retry_due) {
                (Some(a), r) if r.is_none_or(|r| a <= r) => {
                    self.apply_at = None;
                    self.apply(now);
                }
                (_, Some(_)) => {
                    let Some(r) = self.retry.take() else { break };
                    if !self.is_shut_down && self.generation == r.generation && self.route.is_none() {
                        self.build(&r.device_uid, false, now);
                    }
                }
                _ => break,
            }
        }
        self.take_events()
    }

    /// The render loop reported a failed render (the engine is gone). Tears down at once and
    /// asks for a reload, unless the previous death was within `DEATH_WINDOW`. `route_id` is
    /// the failing route's ID: a failure from a route that was already replaced is ignored.
    /// None means "the engine is gone" whatever the route.
    pub fn plugin_died(&mut self, route_id: Option<Uuid>, now: Instant) -> RouteEvents {
        if self.is_shut_down {
            return self.take_events();
        }
        if let Some(id) = route_id
            && Some(id) != self.route_id()
        {
            return self.take_events();
        }
        self.tear_down();
        let repeated = self.last_death.is_some_and(|t| now.saturating_duration_since(t) < Self::DEATH_WINDOW);
        self.last_death = Some(now);
        self.plugin_failed = repeated;
        self.inputs.plugin = if repeated { PluginLoadState::Failed } else { PluginLoadState::Loading };
        self.apply(now);
        if !repeated {
            self.events.reload_plugin = true;
        }
        self.take_events()
    }

    /// Leaves `Failed(Plugin)`: clears the latch and asks for a reload.
    pub fn retry_plugin(&mut self, now: Instant) -> RouteEvents {
        if self.is_shut_down {
            return self.take_events();
        }
        self.plugin_failed = false;
        self.last_death = None;
        self.tear_down();
        self.inputs.plugin = PluginLoadState::Loading;
        self.apply(now);
        self.events.reload_plugin = true;
        self.take_events()
    }

    /// Tears down at once (app quitting). Later input changes do nothing.
    pub fn shut_down(&mut self) -> RouteEvents {
        self.tear_down();
        self.is_shut_down = true;
        self.apply_at = None;
        self.retry = None;
        self.set_state(RouteTarget::Off);
        self.take_events()
    }

    fn take_events(&mut self) -> RouteEvents {
        std::mem::take(&mut self.events)
    }

    fn schedule_apply(&mut self, now: Instant) {
        if self.apply_at.is_none() && !self.is_shut_down {
            self.apply_at = Some(now + Self::COALESCE);
        }
    }

    fn apply(&mut self, now: Instant) {
        if self.is_shut_down {
            return;
        }
        self.generation += 1;
        let mut effective = self.inputs.clone();
        if self.plugin_failed && effective.enabled {
            effective.plugin = PluginLoadState::Failed;
        }
        let target = route_target(&effective);
        let force = std::mem::take(&mut self.force_rebuild);
        if let RouteTarget::Running { device_uid } = &target
            && self.route.as_ref().is_some_and(|(uid, _)| uid == device_uid)
            && !force
        {
            self.set_state(target);
            return;
        }
        self.tear_down();
        match target {
            RouteTarget::Running { device_uid } => self.build(&device_uid, true, now),
            other => self.set_state(other),
        }
    }

    fn build(&mut self, uid: &str, retry: bool, now: Instant) {
        match self.builder.build(uid) {
            Ok(r) => {
                self.route = Some((uid.to_string(), r));
                self.set_state(RouteTarget::Running { device_uid: uid.to_string() });
            }
            Err(_) if !retry => self.set_state(RouteTarget::Failed(RouteFailure::Route)),
            Err(_) => {
                // No route while the retry is pending.
                self.set_state(RouteTarget::Starting);
                self.retry =
                    Some(Retry { at: now + Self::RETRY_DELAY, generation: self.generation, device_uid: uid.to_string() });
            }
        }
    }

    fn tear_down(&mut self) {
        if let Some((_, r)) = self.route.take() {
            r.tear_down();
        }
    }

    fn set_state(&mut self, s: RouteTarget) {
        if s != self.state {
            self.state = s.clone();
            self.events.state_changes.push(s);
        }
    }
}

impl Drop for RouteController {
    /// Never leave the system muted, even if the owner forgets `shut_down`.
    fn drop(&mut self) {
        self.tear_down();
    }
}
