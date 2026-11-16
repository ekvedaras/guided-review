//! A binary run straight from `cargo install` has no app bundle, so macOS shows a blank Dock icon
//! unless the app hands it one while it runs.

use crate::app_icon::{AppIcon, Look};

const SIZE: u16 = 512;

#[expect(
    unsafe_code,
    reason = "objc2 marks the Cocoa call unsafe; it runs on the main thread, proved by the marker, with an image made just above"
)]
/// Also called again when macOS turns light or dark, so the Dock icon matches it.
pub fn show(look: Look) {
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
