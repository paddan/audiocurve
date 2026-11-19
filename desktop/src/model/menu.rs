//! The menu bar menu as data (Swift `MenuPanel`, as a native menu). The tray backend draws
//! it and reports clicks as `MenuAction`s; the model applies them in `AppModel::handle_menu`.

use audiocurve_core::route::{RouteFailure, RouteTarget};
use uuid::Uuid;

use crate::i18n::{tr, trf};
use crate::model::app::AppModel;
use crate::model::status::status_text;
use crate::platform::TrayIconState;

#[derive(Clone, Debug, PartialEq)]
pub enum MenuItem {
    Check {
        title: String,
        checked: bool,
        enabled: bool,
        action: MenuAction,
    },
    /// A disabled line of text.
    Text(String),
    /// A disabled heading.
    Header(String),
    Submenu {
        title: String,
        items: Vec<MenuItem>,
    },
    Separator,
    Action {
        title: String,
        action: MenuAction,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub enum MenuAction {
    ToggleEnabled,
    OpenCaptureSettings,
    LoadProfile(Uuid),
    /// Ticks or unticks the output with this UID in "Active on".
    ToggleOutput(String),
    AssignProfile {
        uid: String,
        profile: Option<Uuid>,
    },
    OpenWindow,
    OpenSettings,
    Quit,
}

/// The menu bar icon for the route (Swift `MenuBarIcon.image(for:)`): running is active, paused
/// or starting is inactive, off or failed is a problem. While a hearing test owns the output
/// (and AudioCurve is on) it is inactive, also when the output is unknown and the route off.
pub fn tray_icon_for(route: &RouteTarget, enabled: bool, testing: bool) -> TrayIconState {
    if enabled && testing {
        return TrayIconState::Inactive;
    }
    match route {
        RouteTarget::Running { .. } => TrayIconState::Active,
        RouteTarget::Paused { .. } | RouteTarget::Starting => TrayIconState::Inactive,
        RouteTarget::Off | RouteTarget::Failed(_) => TrayIconState::Problem,
    }
}

pub fn build_menu(m: &AppModel) -> Vec<MenuItem> {
    let mut menu = vec![
        MenuItem::Check {
            title: tr("AudioCurve on"),
            checked: m.settings.enabled,
            enabled: true,
            action: MenuAction::ToggleEnabled,
        },
        MenuItem::Text(status_text(m).0),
    ];
    if *m.route_state() == RouteTarget::Failed(RouteFailure::Permission) {
        menu.push(MenuItem::Action {
            title: tr("Open System Settings"),
            action: MenuAction::OpenCaptureSettings,
        });
    }
    if let Some(host) = m.other_hosts.first() {
        menu.push(MenuItem::Text(trf(
            "AudioCurve is also running in %@: the sound is processed twice.",
            &[&m.app_name(host)],
        )));
    }
    menu.push(MenuItem::Separator);

    let saved = &m.editor.saved;
    if !saved.is_empty() {
        let mut items = Vec::new();
        if m.editor.source_id.is_none() {
            // Not a choice: it only shows that the loaded profile is not a saved one.
            items.push(MenuItem::Check {
                title: tr("Unsaved profile"),
                checked: true,
                enabled: false,
                action: MenuAction::LoadProfile(Uuid::nil()),
            });
        }
        items.extend(saved.iter().map(|s| MenuItem::Check {
            title: s.name.clone(),
            checked: m.editor.source_id == Some(s.id),
            enabled: true,
            action: MenuAction::LoadProfile(s.id),
        }));
        menu.push(MenuItem::Submenu {
            title: tr("Profile"),
            items,
        });
        menu.push(MenuItem::Separator);
    }

    menu.push(MenuItem::Header(tr("Active on")));
    let devices = m.active_outputs.menu_devices(&m.available_outputs);
    let current = m.current_output.as_ref().map(|d| d.uid.as_str());
    for d in &devices {
        let title = if Some(d.uid.as_str()) == current {
            format!("{} {}", d.name, tr("(current)"))
        } else if !m.available_outputs.iter().any(|o| o.uid == d.uid) {
            format!("{} {}", d.name, tr("(not connected)"))
        } else {
            d.name.clone()
        };
        menu.push(MenuItem::Check {
            title,
            checked: m.active_outputs.is_checked(&d.uid),
            enabled: !m
                .active_outputs
                .is_last_checked(&d.uid, &m.available_outputs),
            action: MenuAction::ToggleOutput(d.uid.clone()),
        });
    }
    menu.push(MenuItem::Separator);

    for d in &devices {
        let assigned = m.assignments.assignment(&d.uid).and_then(|a| a.profile_id);
        let mut items = vec![MenuItem::Check {
            title: "—".into(),
            checked: assigned.is_none(),
            enabled: true,
            action: MenuAction::AssignProfile {
                uid: d.uid.clone(),
                profile: None,
            },
        }];
        items.extend(saved.iter().map(|s| MenuItem::Check {
            title: s.name.clone(),
            checked: assigned == Some(s.id),
            enabled: true,
            action: MenuAction::AssignProfile {
                uid: d.uid.clone(),
                profile: Some(s.id),
            },
        }));
        menu.push(MenuItem::Submenu {
            title: trf("Profile for %@", &[&d.name]),
            items,
        });
    }
    menu.push(MenuItem::Separator);

    for (title, action) in [
        (tr("Open AudioCurve…"), MenuAction::OpenWindow),
        (tr("Settings…"), MenuAction::OpenSettings),
        (tr("Quit AudioCurve"), MenuAction::Quit),
    ] {
        menu.push(MenuItem::Action { title, action });
    }
    menu
}
