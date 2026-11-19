//! The Settings window (Swift `SettingsView`, ⌘,): background processing, the menu bar ear and
//! launch at login, the last only in background mode.

use egui::{
    CentralPanel, Context, Frame, RichText, Ui, ViewportBuilder, ViewportClass, ViewportCommand,
    ViewportId,
};

use crate::i18n::tr;
use crate::model::app::AppModel;
use crate::ui::widgets::secondary;

/// Width of the window, as in Swift.
const WIDTH: f32 = 420.0;
const HEIGHT: f32 = 240.0;

/// Frames in which an opened Settings window takes focus: the opening frame, where the main
/// window's commands still come after it, and the next.
pub const FOCUS_FRAMES: u8 = 2;

/// Shows the Settings window while `open`, as its own viewport; closing it clears `open`. Where
/// the backend cannot open another window, egui shows it as a window inside the main one.
/// While `focus` is above zero the window is brought to the front and made key, once per frame.
pub fn show(ctx: &Context, m: &mut AppModel, open: &mut bool, focus: &mut u8) {
    if !*open {
        *focus = 0;
        return;
    }
    let focusing = *focus > 0;
    if focusing {
        *focus -= 1;
        // The next frame must come even when nothing else asks for one.
        ctx.request_repaint();
    }
    let builder = ViewportBuilder::default()
        .with_title(tr("Settings"))
        .with_inner_size([WIDTH, HEIGHT])
        .with_resizable(false)
        .with_minimize_button(false)
        .with_maximize_button(false);
    ctx.show_viewport_immediate(
        ViewportId::from_hash_of("settings"),
        builder,
        |ui, class| {
            if class == ViewportClass::EmbeddedWindow {
                contents(ui, m);
                return;
            }
            if focusing {
                // Sent from inside the closure, so it goes to this viewport once its window
                // exists. A no-op while the window is key; on a later frame it undoes the main
                // window's commands of the opening frame, which are applied after this one.
                ui.ctx().send_viewport_cmd(ViewportCommand::Focus);
            }
            CentralPanel::default().show(ui, |ui| contents(ui, m));
            if ui.input(|i| i.viewport().close_requested()) {
                *open = false;
            }
        },
    );
}

/// The window's contents: the toggles and their explanations in a grouped form.
pub fn contents(ui: &mut Ui, m: &mut AppModel) {
    ui.set_max_width(WIDTH);
    let explain = |ui: &mut Ui, text: String| {
        let c = secondary(ui.visuals());
        ui.label(RichText::new(text).size(13.0).color(c));
    };
    Frame::group(ui.style())
        .corner_radius(10)
        .inner_margin(12)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            let mut background = m.settings.background_processing;
            if ui
                .checkbox(&mut background, tr("Process all sound in the background"))
                .changed()
            {
                m.set_background_processing(background);
            }
            explain(
                ui,
                tr(if m.settings.background_processing {
                    "AudioCurve keeps processing what your Mac plays when its window is closed."
                } else {
                    "AudioCurve processes sound only while its window is open and Bypass is off. Closing the window quits AudioCurve."
                }),
            );
            ui.separator();
            let mut menu_bar = m.settings.show_in_menu_bar;
            if ui
                .checkbox(&mut menu_bar, tr("Show in menu bar"))
                .changed()
            {
                m.set_show_in_menu_bar(menu_bar);
            }
            if !m.settings.show_in_menu_bar {
                explain(
                    ui,
                    tr("Open AudioCurve from its app icon to get back to the window."),
                );
            }
            if m.mode().offers_launch_at_login() {
                ui.separator();
                let mut login = m.launch_at_login();
                if ui.checkbox(&mut login, tr("Launch at login")).changed() {
                    m.set_launch_at_login(login);
                }
            }
        });
}
