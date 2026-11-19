//! `Tray` on macOS: the menu bar ear and its native menu, with `tray-icon` and `muda`.
//!
//! Each menu item's id is its path from `flatten`, so a click maps back to its `MenuAction`.

use std::collections::HashMap;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};

use objc2::AnyThread;
use objc2_app_kit::{
    NSBitmapImageRep, NSDeviceRGBColorSpace, NSFontWeightRegular, NSGraphicsContext, NSImage,
    NSImageSymbolConfiguration,
};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};
use tray_icon::menu::{
    CheckMenuItem, IsMenuItem, Menu, MenuEvent, MenuItem as NativeItem, PredefinedMenuItem, Submenu,
};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

use super::audio::Waker;
use crate::model::menu::{MenuAction, MenuItem};
use crate::platform::{Tray, TrayIconState};

/// Every item of the menu tree, depth first, with a unique path (its index and title at each
/// level) and its action, if it has one.
pub fn flatten(items: &[MenuItem]) -> Vec<(String, Option<MenuAction>)> {
    let mut out = Vec::new();
    flatten_into(items, "", &mut out);
    out
}

fn flatten_into(items: &[MenuItem], parent: &str, out: &mut Vec<(String, Option<MenuAction>)>) {
    for (i, item) in items.iter().enumerate() {
        let path = path(parent, i, item);
        match item {
            MenuItem::Check { action, .. } | MenuItem::Action { action, .. } => {
                out.push((path, Some(action.clone())))
            }
            MenuItem::Submenu { items, .. } => {
                out.push((path.clone(), None));
                flatten_into(items, &path, out);
            }
            MenuItem::Text(_) | MenuItem::Header(_) | MenuItem::Separator => out.push((path, None)),
        }
    }
}

fn path(parent: &str, index: usize, item: &MenuItem) -> String {
    let title = match item {
        MenuItem::Check { title, .. }
        | MenuItem::Action { title, .. }
        | MenuItem::Submenu { title, .. }
        | MenuItem::Text(title)
        | MenuItem::Header(title) => title.as_str(),
        MenuItem::Separator => "---",
    };
    if parent.is_empty() {
        format!("{index} {title}")
    } else {
        format!("{parent} / {index} {title}")
    }
}

type ActionMap = Arc<Mutex<HashMap<String, MenuAction>>>;

/// The click map for a tray menu: each item's path → its action, plus the app menu's
/// Settings… item, which shares the single `muda` event handler.
pub fn action_map(items: &[MenuItem]) -> HashMap<String, MenuAction> {
    let mut map: HashMap<String, MenuAction> = flatten(items)
        .into_iter()
        .filter_map(|(p, a)| Some((p, a?)))
        .collect();
    map.insert(
        super::app_menu::APP_SETTINGS_ID.to_string(),
        MenuAction::OpenSettings,
    );
    map
}

/// The menu bar item. Create it on the main thread after the event loop has started.
pub struct MacTray {
    icon: TrayIcon,
    actions: ActionMap,
    clicks: Receiver<MenuAction>,
    /// The last menu, to redraw after a click: a native check item toggles itself, and the
    /// model may not change the item.
    items: Vec<MenuItem>,
}

impl MacTray {
    /// `waker` is called after each click is queued. Call it once: `muda` keeps the first
    /// event handler it is given.
    pub fn new(waker: Waker) -> Result<MacTray, String> {
        let actions: ActionMap = Arc::new(Mutex::new(action_map(&[])));
        let (tx, clicks): (Sender<MenuAction>, _) = channel();
        let map = Arc::clone(&actions);
        MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
            // No panics here: this runs inside an AppKit callback.
            let action = map.lock().ok().and_then(|m| m.get(e.id.as_ref()).cloned());
            if let Some(a) = action
                && tx.send(a).is_ok()
            {
                waker();
            }
        }));
        let icon = native_icon(TrayIconState::Inactive)?;
        let icon = TrayIconBuilder::new()
            .with_icon_templated(icon)
            .with_tooltip("AudioCurve")
            .with_menu(Box::new(Menu::new()))
            .build()
            .map_err(|e| e.to_string())?;
        Ok(MacTray {
            icon,
            actions,
            clicks,
            items: Vec::new(),
        })
    }
}

impl Tray for MacTray {
    fn set_menu(&mut self, items: &[MenuItem]) {
        let menu = Menu::new();
        for item in native_items(items, "") {
            log_err(menu.append(item.as_ref()));
        }
        *self.actions.lock().unwrap_or_else(|e| e.into_inner()) = action_map(items);
        self.icon.set_menu(Some(Box::new(menu)));
        self.items = items.to_vec();
    }

    fn set_visible(&mut self, visible: bool) {
        log_err(self.icon.set_visible(visible));
    }

    fn set_icon(&mut self, icon: TrayIconState) {
        match native_icon(icon) {
            Ok(i) => log_err(self.icon.set_icon_templated(Some(i))),
            Err(e) => eprintln!("AudioCurve: menu bar: {e}"),
        }
    }

    fn poll_actions(&mut self) -> Vec<MenuAction> {
        let clicked: Vec<MenuAction> = self.clicks.try_iter().collect();
        if !clicked.is_empty() {
            let items = std::mem::take(&mut self.items);
            self.set_menu(&items);
        }
        clicked
    }
}

fn log_err<E: std::fmt::Display>(r: Result<(), E>) {
    if let Err(e) = r {
        eprintln!("AudioCurve: menu bar: {e}");
    }
}

/// The native items, with the same paths as `flatten` as their ids.
fn native_items(items: &[MenuItem], parent: &str) -> Vec<Box<dyn IsMenuItem>> {
    items
        .iter()
        .enumerate()
        .map(|(i, item)| -> Box<dyn IsMenuItem> {
            let id = path(parent, i, item);
            match item {
                MenuItem::Check {
                    title,
                    checked,
                    enabled,
                    ..
                } => Box::new(CheckMenuItem::with_id(id, title, *enabled, *checked, None)),
                MenuItem::Action { title, .. } => {
                    Box::new(NativeItem::with_id(id, title, true, None))
                }
                MenuItem::Text(title) | MenuItem::Header(title) => {
                    Box::new(NativeItem::with_id(id, title, false, None))
                }
                MenuItem::Separator => Box::new(PredefinedMenuItem::separator()),
                MenuItem::Submenu { title, items } => {
                    let sub = Submenu::with_id(&id, title, true);
                    for child in native_items(items, &id) {
                        log_err(sub.append(child.as_ref()));
                    }
                    Box::new(sub)
                }
            }
        })
        .collect()
}

/// A menu bar for when the real one cannot be made: the app runs without it.
pub struct NoTray;

impl Tray for NoTray {
    fn set_menu(&mut self, _items: &[MenuItem]) {}
    fn set_visible(&mut self, _visible: bool) {}
    fn set_icon(&mut self, _icon: TrayIconState) {}
    fn poll_actions(&mut self) -> Vec<MenuAction> {
        Vec::new()
    }
}

fn native_icon(state: TrayIconState) -> Result<Icon, String> {
    let (rgba, w, h) = icon_rgba(state);
    Icon::from_rgba(rgba, w, h).map_err(|e| e.to_string())
}

const ICON_POINTS: f64 = 18.0;
const ICON_SCALE: f64 = 2.0;

/// The menu bar ear for `state`, as `MenuBarIcon` draws it: SF Symbol `ear.fill` when active,
/// `ear` when inactive, `ear` crossed out on a problem. Black-on-clear RGBA at 2x, for a
/// template image: `(rgba, width, height)`, square.
pub fn icon_rgba(state: TrayIconState) -> (Vec<u8>, u32, u32) {
    let px = (ICON_POINTS * ICON_SCALE) as usize;
    let symbol = match state {
        TrayIconState::Active => "ear.fill",
        TrayIconState::Inactive | TrayIconState::Problem => "ear",
    };
    let mut rgba = render(symbol, px, ICON_POINTS).unwrap_or_else(|| disc(px));
    if state == TrayIconState::Problem {
        cross_out(&mut rgba, px, ICON_SCALE);
    }
    (rgba, px as u32, px as u32)
}

/// `MenuBarIcon.crossedOut`: clears a 3.5 pt gap along the slash from the top-left to the
/// bottom-right corner (1 pt in), then draws the 1.5 pt slash, antialiased. `rgba` is `px`
/// square, top row first, at `scale` pixels per point.
pub fn cross_out(rgba: &mut [u8], px: usize, scale: f64) {
    const GAP: f64 = 3.5;
    const LINE: f64 = 1.5;
    let points = px as f64 / scale;
    let (a, b) = ((1.0, 1.0), (points - 1.0, points - 1.0));
    let coverage = |width: f64, d: f64| (width / 2.0 * scale - d * scale + 0.5).clamp(0.0, 1.0);
    for y in 0..px {
        for x in 0..px {
            let p = ((x as f64 + 0.5) / scale, (y as f64 + 0.5) / scale);
            let d = distance_to_segment(p, a, b);
            let alpha = &mut rgba[(y * px + x) * 4 + 3];
            let kept = f64::from(*alpha) * (1.0 - coverage(GAP, d));
            *alpha = kept.max(255.0 * coverage(LINE, d)).round() as u8;
        }
    }
}

fn distance_to_segment(p: (f64, f64), a: (f64, f64), b: (f64, f64)) -> f64 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let t = (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / (dx * dx + dy * dy)).clamp(0.0, 1.0);
    ((p.0 - a.0 - t * dx).powi(2) + (p.1 - a.1 - t * dy).powi(2)).sqrt()
}

fn render(symbol: &str, px: usize, points: f64) -> Option<Vec<u8>> {
    let config = NSImageSymbolConfiguration::configurationWithPointSize_weight(15.0, unsafe {
        // SAFETY: an AppKit constant.
        NSFontWeightRegular
    });
    let image = NSImage::imageWithSystemSymbolName_accessibilityDescription(
        &NSString::from_str(symbol),
        Some(&NSString::from_str("AudioCurve")),
    )?
    .imageWithSymbolConfiguration(&config)?;
    // SAFETY: null planes make the rep allocate its own buffer; the other arguments describe
    // 8-bit RGBA with no padding requested.
    let rep = unsafe {
        NSBitmapImageRep::initWithBitmapDataPlanes_pixelsWide_pixelsHigh_bitsPerSample_samplesPerPixel_hasAlpha_isPlanar_colorSpaceName_bytesPerRow_bitsPerPixel(
            NSBitmapImageRep::alloc(),
            std::ptr::null_mut(),
            px as isize,
            px as isize,
            8,
            4,
            true,
            false,
            NSDeviceRGBColorSpace,
            0,
            0,
        )
    }?;
    rep.setSize(NSSize::new(points, points));
    let ctx = NSGraphicsContext::graphicsContextWithBitmapImageRep(&rep)?;
    NSGraphicsContext::saveGraphicsState_class();
    NSGraphicsContext::setCurrentContext(Some(&ctx));
    // Centred, at its own size (shrunk if it would not fit).
    let size = image.size();
    let k = (points / size.width.max(size.height)).min(1.0);
    let (w, h) = (size.width * k, size.height * k);
    image.drawInRect(NSRect::new(
        NSPoint::new((points - w) / 2.0, (points - h) / 2.0),
        NSSize::new(w, h),
    ));
    ctx.flushGraphics();
    NSGraphicsContext::restoreGraphicsState_class();

    let data = rep.bitmapData();
    if data.is_null() {
        return None;
    }
    let row = rep.bytesPerRow() as usize;
    let mut rgba = vec![0u8; px * px * 4];
    for y in 0..px {
        for x in 0..px {
            // SAFETY: the rep holds `px` rows of `row` bytes, 4 bytes per pixel.
            let alpha = unsafe { *data.add(y * row + x * 4 + 3) };
            rgba[(y * px + x) * 4 + 3] = alpha;
        }
    }
    Some(rgba)
}

/// A plain dot, if the symbol cannot be drawn.
fn disc(px: usize) -> Vec<u8> {
    let c = px as f64 / 2.0;
    let r = px as f64 * 0.35;
    let mut rgba = vec![0u8; px * px * 4];
    for y in 0..px {
        for x in 0..px {
            let d = ((x as f64 + 0.5 - c).powi(2) + (y as f64 + 0.5 - c).powi(2)).sqrt();
            rgba[(y * px + x) * 4 + 3] = ((r - d + 0.5).clamp(0.0, 1.0) * 255.0) as u8;
        }
    }
    rgba
}
