//! A binary run straight from `cargo install` has no app bundle, so macOS shows a blank Dock icon
//! unless the app hands it one while it runs. A bundle with a compiled icon (`CFBundleIconName`)
//! needs none: macOS draws that icon in the user's icon style, which a picture from the app would
//! cover.

use crate::app_icon::{AppIcon, Look};

const SIZE: u16 = 512;

/// Also called again when macOS turns light or dark, so the Dock icon matches it.
pub fn show(look: Look) {
    if bundle_has_its_own_icon() {
        return;
    }
    paint(look);
}

/// Asks Launch Services for the app's bundle, not `NSBundle::mainBundle`: a launch from Finder runs
/// the bundle's linked binary from its real path, outside the bundle.
fn bundle_has_its_own_icon() -> bool {
    use objc2_app_kit::NSRunningApplication;
    use objc2_foundation::{NSBundle, ns_string};

    NSRunningApplication::currentApplication()
        .bundleURL()
        .and_then(|url| NSBundle::bundleWithURL(&url))
        .is_some_and(|bundle| {
            bundle
                .objectForInfoDictionaryKey(ns_string!("CFBundleIconName"))
                .is_some()
        })
}

#[expect(
    unsafe_code,
    reason = "objc2 marks the Cocoa call unsafe; it runs on the main thread, proved by the marker, with an image made just above"
)]
fn paint(look: Look) {
    use objc2::AllocAnyThread;
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSApplication, NSImage};
    use objc2_foundation::NSData;

    let Some(png) = picture(look) else {
        return;
    };
    let Some(main_thread) = MainThreadMarker::new() else {
        return;
    };
    let data = NSData::with_bytes(&png);
    let Some(image) = NSImage::initWithData(NSImage::alloc(), &data) else {
        return;
    };
    let app = NSApplication::sharedApplication(main_thread);
    unsafe { app.setApplicationIconImage(Some(&image)) };
}

fn picture(look: Look) -> Option<Vec<u8>> {
    AppIcon::running(look)
        .png(SIZE)
        .inspect_err(|error| eprintln!("could not draw the Dock icon: {error:#}"))
        .ok()
}

/// No Dock icon and no turn at the front: for the snapshot run, which draws in the background.
pub fn stay_in_background() {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};

    let Some(main_thread) = MainThreadMarker::new() else {
        return;
    };
    NSApplication::sharedApplication(main_thread)
        .setActivationPolicy(NSApplicationActivationPolicy::Accessory);
}
