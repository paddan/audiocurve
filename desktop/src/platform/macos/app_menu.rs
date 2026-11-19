//! The macOS app menu (the bold "AudioCurve" menu and a Window menu), as the Swift app had:
//! About, Settings… (⌘,), Hide, Hide Others, Show All, Quit, and Minimize/Close. Settings…
//! reaches the model through the tray's menu event handler (`muda` keeps a single handler),
//! keyed by `APP_SETTINGS_ID`.
//!
//! No Edit menu: its ⌘C/⌘V key equivalents would be taken by the menu before egui sees them.

use tray_icon::menu::accelerator::{Accelerator, Code, Modifiers};
use tray_icon::menu::{Menu, MenuItem, PredefinedMenuItem, Submenu};

use crate::i18n::tr;

/// The menu id of the app menu's Settings… item.
pub const APP_SETTINGS_ID: &str = "app.settings";

/// The app menu's item titles in order, for tests (separators left out).
pub fn app_menu_titles() -> Vec<String> {
    vec![
        tr("About AudioCurve"),
        tr("Settings…"),
        tr("Hide AudioCurve"),
        tr("Hide Others"),
        tr("Show All"),
        tr("Quit AudioCurve"),
    ]
}

/// Builds the menu bar and installs it as the app's main menu. Call once, on the main thread,
/// after the event loop has started. The menu lives for the rest of the process.
pub fn install() -> Result<(), String> {
    let t = app_menu_titles();
    let settings = MenuItem::with_id(
        APP_SETTINGS_ID,
        &t[1],
        true,
        Some(Accelerator::new(Modifiers::META, Code::Comma)),
    );
    let app = Submenu::with_items(
        "AudioCurve",
        true,
        &[
            &PredefinedMenuItem::about(Some(&t[0]), None),
            &PredefinedMenuItem::separator(),
            &settings,
            &PredefinedMenuItem::separator(),
            &PredefinedMenuItem::hide(Some(&t[2])),
            &PredefinedMenuItem::hide_others(Some(&t[3])),
            &PredefinedMenuItem::show_all(Some(&t[4])),
            &PredefinedMenuItem::separator(),
            &PredefinedMenuItem::quit(Some(&t[5])),
        ],
    )
    .map_err(|e| e.to_string())?;
    let window = Submenu::with_items(
        tr("Window"),
        true,
        &[
            &PredefinedMenuItem::minimize(Some(&tr("Minimize"))),
            &PredefinedMenuItem::close_window(Some(&tr("Close"))),
        ],
    )
    .map_err(|e| e.to_string())?;
    let menu = Menu::with_items(&[&app, &window]).map_err(|e| e.to_string())?;
    menu.init_for_nsapp();
    window.set_as_windows_menu_for_nsapp();
    // The menu must outlive this function: AppKit holds the native menu, `muda` the handles.
    std::mem::forget(menu);
    std::mem::forget(window);
    Ok(())
}
