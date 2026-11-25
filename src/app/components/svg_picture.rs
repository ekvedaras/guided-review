//! How the app draws an SVG: one set of fonts and one rule for emoji, for a changed image, a
//! diagram and a picture in a summary.

use std::sync::{Arc, LazyLock};

use gpui::{Image, ImageFormat};
use regex::Regex;
use usvg::fontdb::{Database, Family, ID, Query};

use super::palette::{INSTALLED_FONTS, TEXT_FONT};
use crate::image_facts::{self, PixelSize};
use crate::platform::Os;

const SVG_DRAW_SCALE: f32 = 2.;
const MAX_SVG_SIDE_PX: f32 = 4096.;

static OPTIONS: LazyLock<usvg::Options<'static>> = LazyLock::new(|| {
    let mut fonts = INSTALLED_FONTS.clone();
    fonts.set_sans_serif_family(*TEXT_FONT);
    let emoji_face = emoji_face(&fonts);
    let default_fallback = usvg::FontResolver::default_fallback_selector();
    usvg::Options {
        fontdb: Arc::new(fonts),
        font_resolver: usvg::FontResolver {
            select_font: usvg::FontResolver::default_font_selector(),
            select_fallback: Box::new(move |c, used, fonts| match emoji_face {
                Some(face) if is_emoji_glyph(c) && !used.contains(&face) => Some(face),
                _ => default_fallback(c, used, fonts),
            }),
        },
        ..usvg::Options::default()
    }
});

fn emoji_face(fonts: &Database) -> Option<ID> {
    Os::RUNNING.emoji_families().iter().find_map(|family| {
        fonts.query(&Query {
            families: &[Family::Name(family)],
            ..Query::default()
        })
    })
}

/// Whether a character is drawn by the emoji face. The default fallback takes the first font that
/// has the character, which on macOS is `LastResort`, whose glyphs are boxes with a "?". That
/// includes the characters gpui's own rule (`Emoji_Presentation`) leaves out: the picture of 🗑️,
/// ⚠️ or ✔️ is text by default, and the selector after it turns it into an emoji.
pub fn is_emoji_glyph(c: char) -> bool {
    static EMOJI: LazyLock<Regex> = LazyLock::new(|| {
        #[expect(
            clippy::expect_used,
            reason = "the pattern is a constant that compiles"
        )]
        Regex::new(r"[\p{Extended_Pictographic}\u{FE0F}\u{20E3}\u{200D}]").expect("valid pattern")
    });
    EMOJI.is_match(c.encode_utf8(&mut [0; 4]))
}

pub fn parse(svg: &[u8]) -> Result<usvg::Tree, usvg::Error> {
    usvg::Tree::from_data(svg, &OPTIONS)
}

/// The SVG as a picture for gpui, drawn here so its emoji come out right.
pub fn image(svg: &[u8], natural_size: PixelSize) -> Option<Arc<Image>> {
    Some(Arc::new(Image::from_bytes(
        ImageFormat::Png,
        png(svg, natural_size)?,
    )))
}

/// A PNG of the SVG at twice its size, capped so a huge canvas stays small enough to hold.
fn png(svg: &[u8], size: PixelSize) -> Option<Vec<u8>> {
    let tree = parse(svg).ok()?;
    let longest = size.width.max(size.height) as f32;
    let scale = SVG_DRAW_SCALE.min(MAX_SVG_SIDE_PX / longest);
    let (width, height) = (
        image_facts::whole_pixels((size.width as f32 * scale).ceil()),
        image_facts::whole_pixels((size.height as f32 * scale).ceil()),
    );
    let mut pixmap = resvg::tiny_skia::Pixmap::new(width, height)?;
    let fit = resvg::tiny_skia::Transform::from_scale(
        width as f32 / tree.size().width(),
        height as f32 / tree.size().height(),
    );
    resvg::render(&tree, fit, &mut pixmap.as_mut());
    let mut rgba = Vec::with_capacity(pixmap.pixels().len() * 4);
    for pixel in pixmap.pixels() {
        let straight = pixel.demultiply();
        rgba.extend([
            straight.red(),
            straight.green(),
            straight.blue(),
            straight.alpha(),
        ]);
    }
    let mut png = std::io::Cursor::new(Vec::new());
    image::RgbaImage::from_raw(width, height, rgba)?
        .write_to(&mut png, image::ImageFormat::Png)
        .ok()?;
    Some(png.into_inner())
}
