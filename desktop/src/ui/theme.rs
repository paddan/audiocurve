//! The tuned egui look (spec "Global Constraints") and the plot colours, for light and dark.

use egui::{
    Color32, FontData, FontDefinitions, FontFamily, FontId, Frame, Stroke, TextStyle, vec2,
};

/// Left ear (blue).
pub const LEFT: Color32 = Color32::from_rgb(0x46, 0x7D, 0xD7);
/// Right ear (red).
pub const RIGHT: Color32 = Color32::from_rgb(0xD2, 0x5A, 0x5A);
/// The normal-hearing band of the audiogram: green at about 11 % opacity.
pub const NORMAL_BAND: Color32 = Color32::from_rgba_unmultiplied_const(80, 180, 80, 28);
/// Selection fill and links.
pub const ACCENT: Color32 = Color32::from_rgb(0x0A, 0x6E, 0xF5);
/// The ceiling line and the zone above it in the spectrum (macOS system red).
pub const ALERT: Color32 = Color32::from_rgb(255, 59, 48);

/// Positive status text (macOS system green, light and dark).
pub fn positive(v: &egui::Visuals) -> Color32 {
    if v.dark_mode {
        Color32::from_rgb(0x30, 0xD1, 0x58)
    } else {
        Color32::from_rgb(0x28, 0xA7, 0x45)
    }
}

/// Warnings (macOS system orange, light and dark).
pub fn warning(v: &egui::Visuals) -> Color32 {
    if v.dark_mode {
        Color32::from_rgb(0xFF, 0x9F, 0x0A)
    } else {
        Color32::from_rgb(0xE0, 0x80, 0x00)
    }
}

/// The SF font on macOS; elsewhere the file is missing and egui's default font stays.
const SF_PATH: &str = "/System/Library/Fonts/SFNS.ttf";

/// Applies fonts and both the light and the dark style. Call once at startup.
pub fn apply(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    if let Ok(bytes) = std::fs::read(SF_PATH) {
        fonts
            .font_data
            .insert("sf".into(), FontData::from_owned(bytes).into());
        fonts
            .families
            .entry(FontFamily::Proportional)
            .or_default()
            .insert(0, "sf".into());
    }
    ctx.set_fonts(fonts);
    ctx.all_styles_mut(|st| {
        st.text_styles
            .insert(TextStyle::Body, FontId::proportional(14.0));
        st.text_styles
            .insert(TextStyle::Button, FontId::proportional(14.0));
        st.spacing.button_padding = vec2(10.0, 4.0);
        st.spacing.item_spacing = vec2(8.0, 6.0);
        st.spacing.interact_size.y = 24.0;
        let v = &mut st.visuals;
        v.selection.bg_fill = ACCENT;
        v.selection.stroke = Stroke::new(1.0, Color32::WHITE);
        v.hyperlink_color = ACCENT;
        v.slider_trailing_fill = true;
        for w in [
            &mut v.widgets.noninteractive,
            &mut v.widgets.inactive,
            &mut v.widgets.hovered,
            &mut v.widgets.active,
            &mut v.widgets.open,
        ] {
            w.corner_radius = 7.into();
        }
        v.widgets.inactive.bg_stroke = Stroke::NONE;
        v.window_corner_radius = 12.into();
    });
}

/// The frame of the controls panel: rounding 12, inner margin 14.
pub fn panel_frame(style: &egui::Style) -> Frame {
    Frame::group(style).corner_radius(12).inner_margin(14)
}

/// The colour of an ear in the plots.
pub fn ear_color(ear: audiocurve_core::profile::Ear) -> Color32 {
    match ear {
        audiocurve_core::profile::Ear::Left => LEFT,
        audiocurve_core::profile::Ear::Right => RIGHT,
    }
}
