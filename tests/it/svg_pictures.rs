use guided_review::app::components::palette::INSTALLED_FONTS;
use guided_review::app::components::svg_picture::{image, is_emoji_glyph};
use guided_review::image_facts::PixelSize;
use guided_review::platform::Os;

const NATURAL: PixelSize = PixelSize {
    width: 260,
    height: 100,
};

#[test]
fn macos_and_linux_each_name_their_colour_emoji_family() {
    assert!(Os::MacOs.emoji_families().contains(&"Apple Color Emoji"));
    assert!(Os::Linux.emoji_families().contains(&"Noto Color Emoji"));
}

#[test]
fn a_picture_that_is_text_by_default_is_drawn_by_the_emoji_face() {
    for c in ['\u{1F5D1}', '\u{26A0}', '\u{2714}', '\u{2764}'] {
        assert!(is_emoji_glyph(c), "{c:?}");
    }
}

#[test]
fn the_selectors_that_turn_text_into_an_emoji_are_drawn_by_the_emoji_face() {
    for c in ['\u{FE0F}', '\u{20E3}', '\u{200D}'] {
        assert!(is_emoji_glyph(c), "{c:?}");
    }
}

#[test]
fn a_picture_that_is_an_emoji_by_default_is_drawn_by_the_emoji_face() {
    for c in ['\u{2705}', '\u{1F69A}'] {
        assert!(is_emoji_glyph(c), "{c:?}");
    }
}

#[test]
fn letters_and_digits_are_not_drawn_by_the_emoji_face() {
    for c in ['a', 'Z', '7', ' ', 'ä'] {
        assert!(!is_emoji_glyph(c), "{c:?}");
    }
}

/// Black text on white: only a colour emoji has a pixel with colour in it. `LastResort` draws
/// its "?" boxes in black.
fn has_colour(svg: &str) -> bool {
    let picture = image(svg.as_bytes(), NATURAL).expect("the SVG is drawn");
    let pixels = image::load_from_memory(&picture.bytes).unwrap().to_rgba8();
    pixels.pixels().any(|pixel| {
        let [r, g, b, _] = pixel.0;
        r.max(g).max(b) - r.min(g).min(b) > 60
    })
}

fn colour_emoji_font_is_installed() -> bool {
    let installed = Os::RUNNING.emoji_families().iter().any(|family| {
        INSTALLED_FONTS
            .faces()
            .any(|face| face.families.iter().any(|(name, _)| name == family))
    });
    if !installed {
        eprintln!("no colour emoji font on this machine: not checked");
    }
    installed
}

#[test]
fn an_emoji_that_is_text_by_default_is_drawn_in_colour_not_as_a_box() {
    if !colour_emoji_font_is_installed() {
        return;
    }

    assert!(has_colour(include_str!(
        "../fixtures/svg/text-default-emoji.svg"
    )));
}

#[test]
fn an_emoji_that_is_an_emoji_by_default_is_still_drawn_in_colour() {
    if !colour_emoji_font_is_installed() {
        return;
    }

    assert!(has_colour(include_str!(
        "../fixtures/svg/emoji-presentation.svg"
    )));
}
