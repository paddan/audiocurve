//! The one-line status of the route (Swift `HostView.StatusLine`).

use audiocurve_core::route::{RouteFailure, RouteTarget};

use crate::i18n::{tr, trf};
use crate::model::app::AppModel;

/// How the status line is coloured: green, orange, or secondary text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Positive,
    Warning,
    Neutral,
}

pub fn status_text(m: &AppModel) -> (String, Tone) {
    if m.testing {
        return (tr("Hearing test in progress"), Tone::Neutral);
    }
    let name = m.current_output.as_ref().map_or("", |d| d.name.as_str());
    match m.route_state() {
        RouteTarget::Running { .. } => (trf("Active – %@", &[name]), Tone::Positive),
        RouteTarget::Paused { .. } => (trf("Paused – %@ is the output", &[name]), Tone::Warning),
        RouteTarget::Starting => (tr("Starting…"), Tone::Neutral),
        RouteTarget::Off => (tr("Off"), Tone::Neutral),
        RouteTarget::Failed(RouteFailure::Permission) => {
            (tr("Allow AudioCurve to record system audio"), Tone::Warning)
        }
        RouteTarget::Failed(RouteFailure::Plugin) => {
            (tr("The sound processing stopped"), Tone::Warning)
        }
        RouteTarget::Failed(RouteFailure::Route) => {
            (tr("The audio route could not be built"), Tone::Warning)
        }
    }
}
