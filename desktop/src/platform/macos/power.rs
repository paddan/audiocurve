//! System sleep and wake, from `NSWorkspace`'s notification center.

use std::ptr::NonNull;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_app_kit::{
    NSWorkspace, NSWorkspaceDidWakeNotification, NSWorkspaceWillSleepNotification,
};
use objc2_foundation::{NSNotification, NSNotificationCenter, NSObjectProtocol};

/// The registered observers; dropping it removes them.
pub struct PowerObservers {
    center: Retained<NSNotificationCenter>,
    tokens: Vec<Retained<ProtocolObject<dyn NSObjectProtocol>>>,
}

/// Calls `will_sleep` before the Mac sleeps and `did_wake` after a full wake (not a dark wake),
/// on the thread that posts them (the main thread).
pub fn observe_sleep(
    will_sleep: impl Fn() + Send + Sync + 'static,
    did_wake: impl Fn() + Send + Sync + 'static,
) -> PowerObservers {
    let center = NSWorkspace::sharedWorkspace().notificationCenter();
    // SAFETY: reads AppKit's notification name constants.
    let names = unsafe {
        [
            NSWorkspaceWillSleepNotification,
            NSWorkspaceDidWakeNotification,
        ]
    };
    let tokens = [
        RcBlock::new(move |_: NonNull<NSNotification>| will_sleep()),
        RcBlock::new(move |_: NonNull<NSNotification>| did_wake()),
    ]
    .into_iter()
    .zip(names)
    .map(|(block, name)| {
        // SAFETY: no object filter or queue; the block is `Send + Sync`, as the center requires.
        unsafe { center.addObserverForName_object_queue_usingBlock(Some(name), None, None, &block) }
    })
    .collect();
    PowerObservers { center, tokens }
}

impl Drop for PowerObservers {
    fn drop(&mut self) {
        for token in &self.tokens {
            // SAFETY: each token came from this center's `addObserverForName…`.
            unsafe { self.center.removeObserver(token.as_ref()) };
        }
    }
}
