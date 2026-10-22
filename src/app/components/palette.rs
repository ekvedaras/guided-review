use gpui::{Rgba, rgb};

/// GPUI has no usable default font outside Zed, so every text names its family.
pub const TEXT_FONT: &str = "Helvetica Neue";
pub const CODE_FONT: &str = "Menlo";

/// One fixed light palette until the app follows the macOS appearance.
pub struct Palette;

impl Palette {
    pub fn background() -> Rgba {
        rgb(0xffffff)
    }
    pub fn sidebar() -> Rgba {
        rgb(0xf6f8fa)
    }
    pub fn border() -> Rgba {
        rgb(0xd0d7de)
    }
    pub fn text() -> Rgba {
        rgb(0x1f2328)
    }
    pub fn muted_text() -> Rgba {
        rgb(0x59636e)
    }
    pub fn current() -> Rgba {
        rgb(0xddf4ff)
    }
    pub fn removed_line() -> Rgba {
        rgb(0xffebe9)
    }
    pub fn added_line() -> Rgba {
        rgb(0xe6ffec)
    }
}
