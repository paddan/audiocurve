//! `Activity` and `System` on macOS: other apps' playback, launch at login, the Dock icon,
//! System Settings and app names.

use objc2::MainThreadMarker;
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy, NSWorkspace};
use objc2_foundation::{NSFileManager, NSString, NSURL};
use objc2_service_management::{SMAppService, SMAppServiceStatus};

use super::hal;
use crate::i18n::tr;
use crate::platform::{Activity, System};

const CAPTURE_SETTINGS: &str =
    "x-apple.systempreferences:com.apple.settings.PrivacySecurity.extension?Privacy_AudioCapture";

/// Whether another process is playing audio, from the HAL's process objects.
pub struct MacActivity;

impl Activity for MacActivity {
    fn others_playing(&self) -> bool {
        hal::others_playing()
    }
}

pub struct MacSystem;

impl System for MacSystem {
    fn launch_at_login(&self) -> bool {
        // SAFETY: `mainAppService` and `status` take no arguments and have no preconditions.
        unsafe { SMAppService::mainAppService().status() == SMAppServiceStatus::Enabled }
    }

    fn set_launch_at_login(&mut self, on: bool) -> Result<(), String> {
        // SAFETY: as above; registering the main app needs nothing else.
        let result = unsafe {
            let service = SMAppService::mainAppService();
            if on {
                service.registerAndReturnError()
            } else {
                service.unregisterAndReturnError()
            }
        };
        result.map_err(|e| {
            let msg = format!(
                "launch at login {} failed: {}",
                if on { "register" } else { "unregister" },
                e.localizedDescription()
            );
            eprintln!("AudioCurve: {msg}");
            msg
        })
    }

    /// Regular shows the Dock icon; Accessory hides it. Only on the main thread.
    fn set_dock_icon(&mut self, shown: bool) {
        let Some(mtm) = MainThreadMarker::new() else {
            eprintln!("AudioCurve: set_dock_icon called off the main thread");
            return;
        };
        let policy = if shown {
            NSApplicationActivationPolicy::Regular
        } else {
            NSApplicationActivationPolicy::Accessory
        };
        NSApplication::sharedApplication(mtm).setActivationPolicy(policy);
    }

    fn open_capture_settings(&mut self) {
        if let Some(url) = NSURL::URLWithString(&NSString::from_str(CAPTURE_SETTINGS)) {
            NSWorkspace::sharedWorkspace().openURL(&url);
        }
    }

    /// The display name of the app with this bundle ID, or "another app".
    fn app_name(&self, bundle_id: &str) -> String {
        NSWorkspace::sharedWorkspace()
            .URLForApplicationWithBundleIdentifier(&NSString::from_str(bundle_id))
            .and_then(|url| url.path())
            .map(|path| {
                NSFileManager::defaultManager()
                    .displayNameAtPath(&path)
                    .to_string()
                    .replace(".app", "")
            })
            .unwrap_or_else(|| tr("another app"))
    }
}
