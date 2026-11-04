use std::fmt;
use std::io::Cursor;
use std::path::Path;

/// No preview, and no decoding, above this many bytes per side.
pub const MAX_PREVIEW_BYTES: u64 = 10 * 1024 * 1024;

/// A layout size as whole pixels. Callers round or ceil first; below zero and NaN give 0.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the value is clamped to zero or more first, and a float-to-int cast saturates at u32::MAX"
)]
pub fn whole_pixels(value: f32) -> u32 {
    value.max(0.0) as u32
}

pub fn is_image_path(path: &str) -> bool {
    Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            ["png", "jpg", "jpeg", "gif", "webp", "svg"]
                .iter()
                .any(|known| extension.eq_ignore_ascii_case(known))
        })
}

pub fn is_svg_path(path: &str) -> bool {
    Path::new(path)
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("svg"))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PixelSize {
    pub width: u32,
    pub height: u32,
}

impl fmt::Display for PixelSize {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}×{}", self.width, self.height)
    }
}

/// `None` when the bytes are not an image of the kind the path says.
pub fn measure(path: &str, bytes: &[u8]) -> Option<PixelSize> {
    if is_svg_path(path) {
        let tree = usvg::Tree::from_data(bytes, &usvg::Options::default()).ok()?;
        let size = tree.size();
        return Some(PixelSize {
            width: whole_pixels(size.width().round()),
            height: whole_pixels(size.height().round()),
        });
    }
    let (width, height) = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .ok()?
        .into_dimensions()
        .ok()?;
    Some(PixelSize { width, height })
}

/// How an image changed in size: `64×64 → 128×96`, or one size for an added or deleted image.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageChange {
    pub base: Option<PixelSize>,
    pub head: Option<PixelSize>,
}

impl ImageChange {
    pub fn is_known(&self) -> bool {
        self.base.is_some() || self.head.is_some()
    }
}

impl fmt::Display for ImageChange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (self.base, self.head) {
            (Some(base), Some(head)) => write!(f, "{base} → {head}"),
            (Some(only), None) | (None, Some(only)) => write!(f, "{only}"),
            (None, None) => Ok(()),
        }
    }
}
