//! Reads the Swift app's `UserDefaults` once and hands them to `migrate_swift`. The Rust app
//! replaces the Swift one under the same bundle ID, so a hearing test's unrestored volume
//! snapshot and the settings survive the upgrade.

use std::fs;

use audiocurve_core::store::ProfileStore;
use objc2::AnyThread;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_foundation::{NSBundle, NSData, NSNumber, NSString, NSUserDefaults};

use crate::migrate_swift::{SwiftDefaults, migrate};

const BUNDLE_ID: &str = "se.lindefors.audiocurve";
const MARKER: &str = "migrated-swift-defaults";
const PENDING_KEY: &str = "hearingTest.pendingVolume";

/// Migrates once: does nothing when the marker exists in the store root. Removes the Swift
/// pending key after a successful merge and writes the marker last.
pub fn run(store: &ProfileStore) {
    let marker = store.root().join(MARKER);
    if marker.exists() {
        return;
    }
    let defaults = defaults();
    let swift = read(&defaults);
    let confirmed = migrate(&swift, store, store.root());
    if swift.pending_volume.is_some() {
        if !confirmed {
            // Not safely in the store: keep the key and no marker, so the next launch retries.
            return;
        }
        defaults.removeObjectForKey(&NSString::from_str(PENDING_KEY));
    }
    let _ = fs::create_dir_all(store.root());
    let _ = fs::write(marker, b"");
}

/// The app's own domain: the standard defaults when bundled, the suite of that name when run
/// unbundled (`cargo run`).
fn defaults() -> Retained<NSUserDefaults> {
    let bundled = NSBundle::mainBundle()
        .bundleIdentifier()
        .is_some_and(|id| id.to_string() == BUNDLE_ID);
    if bundled {
        return NSUserDefaults::standardUserDefaults();
    }
    suite(BUNDLE_ID)
}

fn suite(name: &str) -> Retained<NSUserDefaults> {
    NSUserDefaults::initWithSuiteName(NSUserDefaults::alloc(), Some(&NSString::from_str(name)))
        .unwrap_or_else(NSUserDefaults::standardUserDefaults)
}

fn read(d: &NSUserDefaults) -> SwiftDefaults {
    let flag = |key: &str| {
        let v: Option<Retained<AnyObject>> = d.objectForKey(&NSString::from_str(key));
        let n = v?.downcast::<NSNumber>().ok()?;
        Some(n.boolValue())
    };
    let pending_volume = d
        .objectForKey(&NSString::from_str(PENDING_KEY))
        .and_then(|v| v.downcast::<NSData>().ok())
        .map(|data| data.to_vec());
    SwiftDefaults {
        enabled: flag("enabled"),
        background_processing: flag("backgroundProcessing"),
        show_in_menu_bar: flag("showInMenuBar"),
        asked_launch_at_login: flag("askedLaunchAtLogin"),
        pending_volume,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reads a throwaway suite, never the real domain, and removes it afterwards.
    #[test]
    fn reads_flags_and_pending_data_from_a_suite() {
        let name = format!("se.lindefors.audiocurve.test.{}", std::process::id());
        let d = suite(&name);
        d.setBool_forKey(false, &NSString::from_str("enabled"));
        d.setBool_forKey(true, &NSString::from_str("askedLaunchAtLogin"));
        let json = br#"[{"deviceUID":"x","scalars":[0,0.5]}]"#;
        let data = NSData::with_bytes(json);
        // SAFETY: NSData is a property-list type, which `setObject:forKey:` accepts.
        unsafe { d.setObject_forKey(Some(&data), &NSString::from_str(PENDING_KEY)) };
        let got = read(&d);
        d.removePersistentDomainForName(&NSString::from_str(&name));
        assert_eq!(got.enabled, Some(false));
        assert_eq!(got.background_processing, None);
        assert_eq!(got.asked_launch_at_login, Some(true));
        assert_eq!(got.pending_volume.as_deref(), Some(&json[..]));
    }
}
