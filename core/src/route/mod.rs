//! The system-wide route: what it should be (`decision`) and the state machine that keeps the
//! built route matching it (`controller`).

pub mod controller;
pub mod decision;

pub use controller::{BuiltRoute, RouteBuildError, RouteBuilder, RouteController, RouteEvents};
pub use decision::{PermissionState, PluginLoadState, RouteFailure, RouteInputs, RouteTarget, route_target};
