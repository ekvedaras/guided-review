//! A binary run straight from `cargo install` has no app bundle, so macOS shows a blank Dock icon
//! unless the app hands it one while it runs.

use crate::app_icon::AppIcon;

const SIZE: u16 = 512;

#[cfg(target_os = "macos")]
#[expect(
    unsafe_code,
    reason = "objc2 marks the Cocoa call unsafe; it runs on the main thread, proved by the marker, with an image made just above"
)]
pub fn show() {
    use objc2::AllocAnyThread;
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSApplication, NSImage};
    use objc2_foundation::NSData;

    let Some(png) = picture() else {
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

#[cfg(not(target_os = "macos"))]
pub fn show() {}

fn picture() -> Option<Vec<u8>> {
    AppIcon::RUNNING
        .png(SIZE)
        .inspect_err(|error| eprintln!("could not draw the Dock icon: {error:#}"))
        .ok()
}

/// No Dock icon and no turn at the front: for the snapshot run, which draws in the background.
#[cfg(target_os = "macos")]
pub fn stay_in_background() {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};

    let Some(main_thread) = MainThreadMarker::new() else {
        return;
    };
    NSApplication::sharedApplication(main_thread)
        .setActivationPolicy(NSApplicationActivationPolicy::Accessory);
}

#[cfg(not(target_os = "macos"))]
pub fn stay_in_background() {}
