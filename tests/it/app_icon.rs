use guided_review::app_icon::{self, AppIcon, Look};
use image::Rgba;

const SIZE: u16 = 64;

/// A spot on the icon's ground, clear of the trail and its dots.
fn ground(icon: AppIcon) -> Rgba<u8> {
    let png = icon.png(SIZE).unwrap();
    let picture = image::load_from_memory(&png).unwrap().to_rgba8();
    *picture.get_pixel(32, 10)
}

fn brightness(Rgba([red, green, blue, _]): Rgba<u8>) -> u16 {
    (u16::from(red) + u16::from(green) + u16::from(blue)) / 3
}

#[test]
fn the_light_icon_stands_on_a_light_ground() {
    assert!(brightness(ground(AppIcon::release(Look::Light))) > 200);
}

#[test]
fn the_dark_icon_stands_on_a_dark_ground() {
    assert!(brightness(ground(AppIcon::release(Look::Dark))) < 60);
}

#[test]
fn a_dev_build_shows_the_icon_of_the_look_it_is_given() {
    assert_eq!(
        ground(AppIcon::dev(Look::Light)),
        ground(AppIcon::release(Look::Light))
    );
    assert_eq!(
        ground(AppIcon::dev(Look::Dark)),
        ground(AppIcon::release(Look::Dark))
    );
}

#[test]
fn a_dev_build_wears_a_badge_the_release_does_not() {
    let release = AppIcon::release(Look::Light).png(SIZE).unwrap();
    let dev = AppIcon::dev(Look::Light).png(SIZE).unwrap();
    assert_ne!(release, dev);
}

#[test]
fn the_pictures_are_exported_from_the_icon_composer_icon_as_it_is() {
    assert_eq!(
        app_icon::icon_composer_digest(),
        app_icon::digest_the_pictures_were_exported_from(),
        "AppIcon.icon changed after its pictures were exported: run scripts/app-icon-pictures"
    );
}
