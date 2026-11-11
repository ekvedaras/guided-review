//! A binary run straight from `cargo install` has no app bundle, so macOS shows a blank Dock icon
//! unless the app hands it one while it runs.

use crate::app::assets::Assets;

const SIZE: u32 = 512;

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
    let tree = if cfg!(debug_assertions) {
        let mut options = usvg::Options::default();
        options.fontdb_mut().load_system_fonts();
        usvg::Tree::from_str(Assets::DEV_APP_ICON, &options).ok()?
    } else {
        usvg::Tree::from_str(Assets::APP_ICON, &usvg::Options::default()).ok()?
    };
    let mut pixmap = resvg::tiny_skia::Pixmap::new(SIZE, SIZE)?;
    let scale = SIZE as f32 / tree.size().width();
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    pixmap.encode_png().ok()
}
