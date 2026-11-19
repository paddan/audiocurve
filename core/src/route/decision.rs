//! What the system-wide route should be (Swift `RouteDecision`).

use crate::outputs::ActiveOutputs;

/// Whether the user has allowed recording system audio. `Unknown` means "not seen denied yet".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PermissionState {
    Granted,
    Denied,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PluginLoadState {
    Loading,
    Loaded,
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RouteFailure {
    Permission,
    Plugin,
    Route,
}

/// Only `Running` has a route; in every other state the system plays sound normally.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RouteTarget {
    Off,
    Starting,
    Paused { device_uid: String },
    Running { device_uid: String },
    Failed(RouteFailure),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RouteInputs {
    pub enabled: bool,
    pub permission: PermissionState,
    pub plugin: PluginLoadState,
    /// The system's default output device, None if it cannot be read.
    pub output_uid: Option<String>,
    pub active_outputs: ActiveOutputs,
    /// A hearing test is running; the test owns the output until it ends.
    pub testing: bool,
}

/// The target route for `inputs`. First match wins: off, testing (paused on the output, off if
/// the output is unknown), denied permission, plugin failed, plugin loading, unknown output,
/// output not ticked in "Active on", else running.
pub fn route_target(inputs: &RouteInputs) -> RouteTarget {
    if !inputs.enabled {
        return RouteTarget::Off;
    }
    if inputs.testing {
        return match &inputs.output_uid {
            Some(uid) => RouteTarget::Paused { device_uid: uid.clone() },
            None => RouteTarget::Off,
        };
    }
    if inputs.permission == PermissionState::Denied {
        return RouteTarget::Failed(RouteFailure::Permission);
    }
    match inputs.plugin {
        PluginLoadState::Failed => return RouteTarget::Failed(RouteFailure::Plugin),
        PluginLoadState::Loading => return RouteTarget::Starting,
        PluginLoadState::Loaded => {}
    }
    let Some(uid) = &inputs.output_uid else { return RouteTarget::Failed(RouteFailure::Route) };
    if inputs.active_outputs.is_active(Some(uid)) {
        RouteTarget::Running { device_uid: uid.clone() }
    } else {
        RouteTarget::Paused { device_uid: uid.clone() }
    }
}
