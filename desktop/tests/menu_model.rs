mod support;

use std::time::Instant;

use audiocurve_app::model::app::{AppModel, Launch, UiRequest};
use audiocurve_app::model::menu::{MenuAction, MenuItem, build_menu};
use audiocurve_app::platform::fake::{FakeControl, fake};
use audiocurve_core::route::RouteFailure;
use audiocurve_core::store::ProfileStore;
use support::example_profile;

fn start() -> (tempfile::TempDir, AppModel, FakeControl, Instant) {
    let dir = tempfile::tempdir().unwrap();
    let store = ProfileStore::new(dir.path().to_path_buf());
    let (p, c) = fake();
    let t = Instant::now();
    let m = AppModel::new(
        p,
        store,
        Launch {
            from_login_item: false,
        },
        t,
    );
    (dir, m, c, t)
}

/// Ticks at each deadline until there is none or 5 s have passed.
fn settle(m: &mut AppModel, t: &mut Instant) {
    let end = *t + std::time::Duration::from_secs(5);
    m.tick(*t);
    while let Some(d) = m.next_deadline().filter(|d| *d <= end) {
        *t = d.max(*t);
        m.tick(*t);
    }
}

/// A one-word description of each item: its kind and title.
fn shape(items: &[MenuItem]) -> Vec<String> {
    items
        .iter()
        .map(|i| match i {
            MenuItem::Check { title, checked, .. } => {
                format!("check {} {title}", if *checked { "on" } else { "off" })
            }
            MenuItem::Text(t) => format!("text {t}"),
            MenuItem::Header(t) => format!("header {t}"),
            MenuItem::Submenu { title, .. } => format!("submenu {title}"),
            MenuItem::Separator => "---".into(),
            MenuItem::Action { title, .. } => format!("action {title}"),
        })
        .collect()
}

fn submenu<'a>(items: &'a [MenuItem], name: &str) -> &'a [MenuItem] {
    items
        .iter()
        .find_map(|i| match i {
            MenuItem::Submenu { title, items } if title == name => Some(items.as_slice()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no submenu {name}"))
}

fn check<'a>(items: &'a [MenuItem], name: &str) -> (&'a MenuItem, bool, bool) {
    items
        .iter()
        .find_map(|i| match i {
            MenuItem::Check {
                title,
                checked,
                enabled,
                ..
            } if title == name => Some((i, *checked, *enabled)),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no check item {name}"))
}

#[test]
fn menu_has_the_sketched_layout() {
    let (_d, mut m, _c, mut t) = start();
    settle(&mut m, &mut t);
    let menu = build_menu(&m);
    assert_eq!(
        shape(&menu),
        [
            "check on AudioCurve on",
            "text Active – AirPods Pro",
            "---",
            "header Active on",
            "check on AirPods Pro (current)",
            "check on Hearing Aids",
            "check on Mac Studio Speakers",
            "---",
            "submenu Profile for AirPods Pro",
            "submenu Profile for Hearing Aids",
            "submenu Profile for Mac Studio Speakers",
            "---",
            "action Open AudioCurve…",
            "action Settings…",
            "action Quit AudioCurve",
        ]
    );
}

#[test]
fn saved_profiles_add_the_profile_submenu_with_an_unsaved_entry() {
    let (_d, mut m, _c, t) = start();
    m.editor.set_profile(example_profile(), t);
    m.editor.save_as("Example");
    m.editor.source_id = None;
    let menu = build_menu(&m);
    let titles = shape(&menu);
    assert_eq!(titles[2], "---");
    assert_eq!(titles[3], "submenu Profile");
    assert_eq!(titles[4], "---");
    let profile = submenu(&menu, "Profile");
    assert_eq!(
        shape(profile),
        ["check on Unsaved profile", "check off Example"]
    );
}

#[test]
fn last_checked_output_is_disabled() {
    let (_d, mut m, _c, t) = start();
    let devices = m.available_outputs.clone();
    m.set_output_enabled(&devices[1], false, t);
    m.set_output_enabled(&devices[2], false, t);
    let menu = build_menu(&m);
    assert!(!check(&menu, "AirPods Pro (current)").2);
    assert!(check(&menu, "AirPods Pro (current)").1);
    assert!(check(&menu, "Hearing Aids").2);
    assert!(!check(&menu, "Hearing Aids").1);
}

#[test]
fn remembered_output_is_marked_not_connected() {
    let (_d, mut m, c, t) = start();
    m.tick(t);
    let all = m.available_outputs.clone();
    // Remember the hearing aids in an explicit selection, then disconnect them.
    m.set_output_enabled(&all[2], false, t);
    c.set_outputs(vec![all[0].clone(), all[2].clone()], Some("airpods"));
    c.emit(audiocurve_app::platform::PlatformEvent::OutputsChanged);
    m.tick(t);
    let menu = build_menu(&m);
    assert!(
        shape(&menu)
            .iter()
            .any(|s| s == "check on Hearing Aids (not connected)")
    );
}

#[test]
fn permission_item_only_when_denied() {
    let (_d, mut m, c, t) = start();
    m.tick(t);
    assert!(!shape(&build_menu(&m)).contains(&"action Open System Settings".to_string()));
    c.emit(audiocurve_app::platform::PlatformEvent::CaptureLooksDenied);
    let mut now = t;
    for _ in 0..50 {
        now += std::time::Duration::from_millis(100);
        m.tick(now);
        if *m.route_state() == audiocurve_core::route::RouteTarget::Failed(RouteFailure::Permission)
        {
            break;
        }
    }
    let s = shape(&build_menu(&m));
    assert_eq!(s[1], "text Allow AudioCurve to record system audio");
    assert_eq!(s[2], "action Open System Settings");
}

#[test]
fn other_host_warning_appears_with_the_app_name() {
    let (_d, mut m, _c, _t) = start();
    m.other_hosts = vec!["com.apple.logic10".into()];
    let s = shape(&build_menu(&m));
    assert!(s.contains(
        &"text AudioCurve is also running in logic10: the sound is processed twice.".to_string()
    ));
}

#[test]
fn profile_submenu_checks_the_assigned_profile() {
    let (_d, mut m, _c, t) = start();
    m.editor.set_profile(example_profile(), t);
    m.editor.save_as("Example");
    let id = m.editor.saved[0].id;
    let aids = m.available_outputs[1].clone();
    m.assign(&aids, Some(id));
    let menu = build_menu(&m);
    assert_eq!(
        shape(submenu(&menu, "Profile for Hearing Aids")),
        ["check off —", "check on Example"]
    );
    assert_eq!(
        shape(submenu(&menu, "Profile for AirPods Pro")),
        ["check on —", "check off Example"]
    );
    let sub = submenu(&menu, "Profile for Hearing Aids");
    let MenuItem::Check { action, .. } = &sub[1] else {
        unreachable!()
    };
    assert_eq!(
        *action,
        MenuAction::AssignProfile {
            uid: "aids".into(),
            profile: Some(id)
        }
    );
}

#[test]
fn clicking_items_changes_the_model() {
    let (_d, mut m, c, t) = start();
    m.editor.set_profile(example_profile(), t);
    m.editor.save_as("Example");
    let id = m.editor.saved[0].id;
    m.tick(t);

    c.click(MenuAction::ToggleOutput("aids".into()));
    m.tick(t);
    assert!(!m.active_outputs.is_checked("aids"));
    c.click(MenuAction::ToggleOutput("aids".into()));
    m.tick(t);
    assert!(m.active_outputs.is_checked("aids"));

    c.click(MenuAction::AssignProfile {
        uid: "speakers".into(),
        profile: Some(id),
    });
    m.tick(t);
    assert_eq!(
        m.assignments.assignment("speakers").unwrap().profile_id,
        Some(id)
    );
    c.click(MenuAction::AssignProfile {
        uid: "speakers".into(),
        profile: None,
    });
    m.tick(t);
    assert!(m.assignments.assignment("speakers").is_none());

    m.editor.source_id = None;
    c.click(MenuAction::LoadProfile(id));
    m.tick(t);
    assert_eq!(m.editor.source_id, Some(id));

    c.click(MenuAction::ToggleEnabled);
    m.tick(t);
    assert!(!m.settings.enabled);
    assert_eq!(
        shape(&c.menu())[0],
        "check off AudioCurve on",
        "the tray gets the new menu"
    );

    c.click(MenuAction::OpenWindow);
    c.click(MenuAction::OpenSettings);
    m.tick(t);
    assert_eq!(
        m.take_ui_requests(),
        [UiRequest::OpenWindow, UiRequest::OpenSettings]
    );
    assert!(m.take_ui_requests().is_empty());

    c.click(MenuAction::Quit);
    m.tick(t);
    assert!(m.is_shut_down());
}
