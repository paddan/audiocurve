//! The AudioCurve egui app. On macOS it runs on the real platform: system audio through the
//! route, the menu bar ear, and the app lifecycle of Swift's `AppDelegate`. Elsewhere it runs on
//! the fake platform: demo outputs and a fake route fed with noise in real time, over a store
//! of its own in the temporary directory.

use std::time::Instant;

use audiocurve_app::i18n::{self, Lang, tr};
use audiocurve_app::lifecycle::{self, CloseAction};
use audiocurve_app::model::app::{AppModel, UiRequest};
use audiocurve_app::ui::{main_window, settings, theme};
use audiocurve_core::store::ProfileStore;
use egui::{
    Align, CentralPanel, Id, Key, Layout, Modal, Modifiers, RichText, ViewportBuilder,
    ViewportCommand,
};

struct App {
    m: AppModel,
    settings_open: bool,
    /// Frames left in which the Settings window takes focus (opened, or opened again while
    /// behind).
    focus_settings: u8,
    /// The main window is shown (the app sends every show and hide itself).
    window_visible: bool,
    /// The first frame hides the window for a hidden start.
    start_hidden: bool,
    launched: Instant,
    /// `NSApplication.isActive` at the last tick, for the reopen edge.
    was_active: bool,
    ask_launch_at_login: bool,
    #[cfg(target_os = "macos")]
    quit_signal: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> App {
        theme::apply(&cc.egui_ctx);
        #[cfg(target_os = "macos")]
        let (platform, launch, quit_signal) = mac::platform(&cc.egui_ctx);
        #[cfg(not(target_os = "macos"))]
        let (platform, launch) = fake_platform();
        let visible = lifecycle::starts_visible(launch.from_login_item);
        let now = Instant::now();
        #[cfg(target_os = "macos")]
        let store = ProfileStore::standard();
        // The demo platform's made-up outputs must never reach the real store, where they
        // would become the only active output once a real backend uses it.
        #[cfg(not(target_os = "macos"))]
        let store = ProfileStore::new(std::env::temp_dir().join("audiocurve-demo"));
        // The Swift app's settings and unrestored volume, once, before the model reads them.
        #[cfg(target_os = "macos")]
        audiocurve_app::platform::macos::migrate::run(&store);
        let mut m = AppModel::new(platform, store, launch, now);
        let ask_launch_at_login = m.take_launch_at_login_question();
        let mut app = App {
            m,
            settings_open: false,
            focus_settings: 0,
            window_visible: true,
            start_hidden: !visible,
            launched: now,
            was_active: false,
            ask_launch_at_login,
            #[cfg(target_os = "macos")]
            quit_signal,
        };
        if visible {
            app.show_window(&cc.egui_ctx);
        }
        app
    }

    /// Shows and focuses the main window, with the Dock icon.
    fn show_window(&mut self, ctx: &egui::Context) {
        ctx.send_viewport_cmd(ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(ViewportCommand::Focus);
        self.window_visible = true;
        self.m.set_window_open(true);
    }

    /// Hides the main window; in background mode the Dock icon goes too.
    fn hide_window(&mut self, ctx: &egui::Context) {
        ctx.send_viewport_cmd(ViewportCommand::Visible(false));
        self.window_visible = false;
        // A child viewport goes with its hidden parent.
        self.settings_open = false;
        self.m.set_window_open(false);
        #[cfg(target_os = "macos")]
        mac::deactivate();
    }

    /// The main window's close button: hides in background mode, quits otherwise. The fake
    /// platform has no menu bar item to come back through, so there it always quits.
    fn handle_close_request(&mut self, ctx: &egui::Context, now: Instant) {
        if self.m.is_shut_down() || !ctx.input(|i| i.viewport().close_requested()) {
            return;
        }
        let action = if cfg!(target_os = "macos") {
            lifecycle::on_close_request(self.m.mode())
        } else {
            CloseAction::Quit
        };
        self.m.hearing_window_closed(now);
        match action {
            CloseAction::Hide => {
                ctx.send_viewport_cmd(ViewportCommand::CancelClose);
                self.hide_window(ctx);
            }
            // Tears the route down before the process ends; the close goes ahead.
            CloseAction::Quit => self.m.quit(),
        }
    }

    /// A Dock click or a Finder double-click while hidden activates the app: show the window.
    fn check_reopen(&mut self, ctx: &egui::Context, now: Instant) {
        #[cfg(target_os = "macos")]
        let active = mac::is_active();
        #[cfg(not(target_os = "macos"))]
        let active = false;
        let became_active = active && !self.was_active;
        self.was_active = active;
        let settled = now.saturating_duration_since(self.launched) > lifecycle::LAUNCH_SETTLE;
        if lifecycle::should_reopen(self.window_visible, became_active, settled) {
            self.show_window(ctx);
        }
    }

    /// Ticks at the model's next deadline, and more often while meters show or while hidden.
    fn schedule_repaint(&mut self, ctx: &egui::Context, now: Instant) {
        let until = self
            .m
            .next_deadline()
            .map(|d| d.saturating_duration_since(now));
        let meters = self.m.hearing().is_none();
        ctx.request_repaint_after(lifecycle::next_repaint(until, self.window_visible, meters));
    }

    /// The one-time "Start AudioCurve when you log in?" (Swift `askLaunchAtLoginOnce`).
    fn launch_at_login_question(&mut self, ctx: &egui::Context) {
        let mut answer = None;
        let shown = Modal::new(Id::unique("launch_at_login")).show(ctx, |ui| {
            ui.set_max_width(320.0);
            ui.label(RichText::new(tr("Start AudioCurve when you log in?")).strong());
            ui.add_space(4.0);
            ui.label(tr(
                "AudioCurve processes all sound in the background. You can change this later in Settings.",
            ));
            ui.add_space(8.0);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.button(tr("Start at Login")).clicked() {
                    answer = Some(true);
                }
                if ui.button(tr("Not Now")).clicked() {
                    answer = Some(false);
                }
            });
        });
        if answer.is_none() && shown.should_close() {
            answer = Some(false);
        }
        if let Some(on) = answer {
            self.ask_launch_at_login = false;
            if on {
                self.m.set_launch_at_login(true);
            }
        }
    }
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let now = Instant::now();
        if std::mem::take(&mut self.start_hidden) {
            self.hide_window(ctx);
        }
        #[cfg(target_os = "macos")]
        if self.quit_signal.load(std::sync::atomic::Ordering::Relaxed) {
            self.m.quit();
        }
        self.m.tick(now);
        for request in self.m.take_ui_requests() {
            if request == UiRequest::OpenSettings {
                // Settings is a child of the main window, so a hidden main window is shown too.
                // On macOS `Visible(true)` is `makeKeyAndOrderFront`, applied after Settings'
                // own commands, so it is sent only when needed and Settings refocuses after it.
                self.settings_open = true;
                self.focus_settings = settings::FOCUS_FRAMES;
                if !self.window_visible {
                    ctx.send_viewport_cmd(ViewportCommand::Visible(true));
                    self.window_visible = true;
                    self.m.set_window_open(true);
                }
            } else {
                self.show_window(ctx);
            }
        }
        self.handle_close_request(ctx, now);
        self.check_reopen(ctx, now);
        if self.m.is_shut_down() {
            ctx.send_viewport_cmd(ViewportCommand::Close);
            return;
        }
        self.schedule_repaint(ctx, now);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let now = Instant::now();
        if ui.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::Comma)) {
            self.settings_open = true;
            self.focus_settings = settings::FOCUS_FRAMES;
        }
        CentralPanel::default().show(ui, |ui| main_window::show(ui, &mut self.m, now));
        settings::show(
            ui.ctx(),
            &mut self.m,
            &mut self.settings_open,
            &mut self.focus_settings,
        );
        if self.ask_launch_at_login {
            self.launch_at_login_question(ui.ctx());
        }
    }

    fn on_exit(&mut self) {
        self.m.quit();
    }
}

#[cfg(not(target_os = "macos"))]
fn fake_platform() -> (
    audiocurve_app::platform::Platform,
    audiocurve_app::model::app::Launch,
) {
    let (platform, control) = audiocurve_app::platform::fake::fake();
    // Before the first route is built, so the meters move.
    control.set_realtime(true);
    let launch = audiocurve_app::model::app::Launch {
        from_login_item: false,
    };
    (platform, launch)
}

fn main() -> eframe::Result {
    let locale = sys_locale::get_locale().unwrap_or_default();
    i18n::set_lang(Lang::from_locale(&locale));
    #[cfg(target_os = "macos")]
    if let Some(mode) = std::env::var_os("AUDIOCURVE_TEST_ROUTE_THEN_WAIT") {
        mac::route_then_wait(&mode.to_string_lossy());
    }
    #[allow(unused_mut)]
    let mut options = eframe::NativeOptions {
        viewport: ViewportBuilder::default()
            .with_title("AudioCurve")
            .with_inner_size([900.0, 720.0])
            .with_min_inner_size(main_window::MIN_WINDOW_SIZE),
        ..Default::default()
    };
    #[cfg(target_os = "macos")]
    mac::configure(&mut options);
    eframe::run_native(
        "AudioCurve",
        options,
        Box::new(|cc| Ok(Box::new(App::new(cc)))),
    )
}

/// The macOS side of the lifecycle: the event loop's activation policy, the login item check,
/// the app's activation, signals and the panic teardown.
#[cfg(target_os = "macos")]
mod mac {
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;

    use audiocurve_app::engine::{Engine, SharedEngine};
    use audiocurve_app::model::app::Launch;
    use audiocurve_app::platform::macos::audio::{EventQueue, Waker};
    use audiocurve_app::platform::macos::route::{
        MacRouteBuilder, is_render_thread, tear_down_registered_route,
    };
    use audiocurve_app::platform::macos::tray::{MacTray, NoTray};
    use audiocurve_app::platform::macos::{hal, mac_platform};
    use audiocurve_app::platform::{Platform, Tray};
    use objc2::rc::Retained;
    use objc2::{MainThreadMarker, msg_send};
    use objc2_app_kit::NSApplication;
    use objc2_foundation::{NSAppleEventDescriptor, NSAppleEventManager};
    use signal_hook::consts::{SIGHUP, SIGINT, SIGTERM};
    use signal_hook::iterator::Signals;
    use winit::platform::macos::{ActivationPolicy, EventLoopBuilderExtMacOS};

    const fn four_cc(code: &[u8; 4]) -> u32 {
        u32::from_be_bytes(*code)
    }
    const K_AE_OPEN_APPLICATION: u32 = four_cc(b"oapp");
    const KEY_AE_PROP_DATA: u32 = four_cc(b"prdt");
    const KEY_AE_LAUNCHED_AS_LOGIN_ITEM: u32 = four_cc(b"lgit");

    /// Every launch starts as an accessory app that does not take focus; the model then sets
    /// the Dock icon, and a visible start focuses its window. Whether this is a login item
    /// launch is only known once the app has launched.
    pub fn configure(options: &mut eframe::NativeOptions) {
        // Without an icon eframe sets its egui logo as the app icon at runtime, which hides
        // the bundle's `AppIcon.icns` in the Dock; an empty one leaves the bundle's.
        options.viewport.icon = Some(std::sync::Arc::new(egui::IconData::default()));
        options.event_loop_builder = Some(Box::new(|builder| {
            builder
                .with_activation_policy(ActivationPolicy::Accessory)
                .with_activate_ignoring_other_apps(false)
                // Our own app menu (`app_menu::install`) replaces winit's default one.
                .with_default_menu(false);
        }));
    }

    /// The real platform, the launch and the quit-signal flag. On the main thread, in the
    /// creation callback (the event loop runs; the launch's Apple event is current).
    pub fn platform(ctx: &egui::Context) -> (Platform, Launch, Arc<AtomicBool>) {
        let waker: Waker = {
            let ctx = ctx.clone();
            Arc::new(move || ctx.request_repaint())
        };
        audiocurve_app::platform::macos::diag::enable();
        audiocurve_app::platform::macos::diag::log("AudioCurve started");
        install_panic_teardown(|| {});
        let quit = quit_on_signals(waker.clone(), || {});
        let tray: Box<dyn Tray> = match MacTray::new(waker.clone()) {
            Ok(t) => Box::new(t),
            Err(e) => {
                eprintln!("AudioCurve: no menu bar item: {e}");
                Box::new(NoTray)
            }
        };
        // After the tray: the app menu's Settings… goes through the tray's event handler.
        if let Err(e) = audiocurve_app::platform::macos::app_menu::install() {
            eprintln!("AudioCurve: no app menu: {e}");
        }
        let launch = Launch {
            from_login_item: launched_as_login_item(),
        };
        (mac_platform(waker, tray), launch, quit)
    }

    /// Swift `AppDelegate.launchedAsLoginItem`: the launch's open-application Apple event says
    /// it came from the login item.
    fn launched_as_login_item() -> bool {
        let Some(event) = NSAppleEventManager::sharedAppleEventManager().currentAppleEvent() else {
            return false;
        };
        // SAFETY: `eventID` returns an `AEEventID` (a `u32`) and takes no arguments.
        let id: u32 = unsafe { msg_send![&*event, eventID] };
        // SAFETY: `paramDescriptorForKeyword:` takes an `AEKeyword` (a `u32`) and returns a
        // descriptor or nil.
        let data: Option<Retained<NSAppleEventDescriptor>> =
            unsafe { msg_send![&*event, paramDescriptorForKeyword: KEY_AE_PROP_DATA] };
        id == K_AE_OPEN_APPLICATION
            && data.is_some_and(|d| d.enumCodeValue() == KEY_AE_LAUNCHED_AS_LOGIN_ITEM)
    }

    /// `NSApplication.isActive`; false off the main thread.
    pub fn is_active() -> bool {
        MainThreadMarker::new().is_some_and(|mtm| NSApplication::sharedApplication(mtm).isActive())
    }

    /// Gives the focus back after hiding, so activating the app again is seen as a reopen.
    pub fn deactivate() {
        if let Some(mtm) = MainThreadMarker::new() {
            NSApplication::sharedApplication(mtm).deactivate();
        }
    }

    /// SIGTERM, SIGINT and SIGHUP set the returned flag and wake the UI, which then quits
    /// (tearing the route down). A second signal (the UI may be hung) tears the live route
    /// down from this thread and ends the process the default way. `after_teardown` is for the
    /// test mode.
    fn quit_on_signals(waker: Waker, after_teardown: fn()) -> Arc<AtomicBool> {
        let flag = Arc::new(AtomicBool::new(false));
        match Signals::new([SIGTERM, SIGINT, SIGHUP]) {
            Ok(mut signals) => {
                let set = flag.clone();
                let spawned = std::thread::Builder::new()
                    .name("audiocurve-signals".into())
                    .spawn(move || {
                        for signal in signals.forever() {
                            if set.swap(true, Ordering::Relaxed) {
                                tear_down_registered_route();
                                after_teardown();
                                let _ = signal_hook::low_level::emulate_default_handler(signal);
                            }
                            waker();
                        }
                    });
                if let Err(e) = spawned {
                    eprintln!("AudioCurve: no signal thread: {e}");
                }
            }
            Err(e) => eprintln!("AudioCurve: signals not handled: {e}"),
        }
        flag
    }

    /// A panic anywhere but in an IO block (which catches its own and goes silent) tears the
    /// live route down, so the Mac is not left muted, and then aborts. `after_teardown` is for
    /// the test mode.
    fn install_panic_teardown(after_teardown: fn()) {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            if is_render_thread() {
                previous(info);
                return;
            }
            tear_down_registered_route();
            after_teardown();
            previous(info);
            std::process::abort();
        }));
    }

    /// Test mode (`AUDIOCURVE_TEST_ROUTE_THEN_WAIT`): builds an unmuted route on the default
    /// output, waits for a quit signal, tears the route down and exits with success. Prints the
    /// own routes this process sees after the build and after the teardown. With the value
    /// `panic` it panics instead of waiting, for the panic hook to tear the route down; with
    /// `hang` the main thread never checks the quit flag, so only a second signal ends it.
    pub fn route_then_wait(mode: &str) -> ! {
        install_panic_teardown(|| println!("panicked: own routes {}", own_routes_settled()));
        let quit = quit_on_signals(Arc::new(|| {}), || {
            println!("second signal: own routes {}", own_routes_settled())
        });
        let uid = hal::default_output().and_then(hal::device_uid);
        let Some(uid) = uid else {
            eprintln!("AudioCurve: no default output");
            std::process::exit(2);
        };
        let engine: SharedEngine = Rc::new(RefCell::new(Arc::new(Engine::new(48_000.0, 4096))));
        let mut builder = MacRouteBuilder::new(engine, EventQueue::new(Arc::new(|| {})), false);
        let route = match builder.build_route(&uid) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("AudioCurve: {}", e.0);
                std::process::exit(3);
            }
        };
        println!("route ready: own routes {}", own_routes());
        match mode {
            "panic" => panic!("test-mode panic with a live route"),
            "hang" => loop {
                std::thread::sleep(Duration::from_secs(60));
            },
            _ => {}
        }
        while !quit.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(20));
        }
        drop(route);
        println!("torn down: own routes {}", own_routes_settled());
        std::process::exit(0);
    }

    /// `own_routes` once the HAL has updated its device list after a destroy (up to 1 s).
    fn own_routes_settled() -> usize {
        for _ in 0..50 {
            if own_routes() == 0 {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        own_routes()
    }

    fn own_routes() -> usize {
        hal::devices()
            .into_iter()
            .filter_map(hal::device_uid)
            .filter(|uid| hal::is_own_route(uid))
            .count()
    }
}
