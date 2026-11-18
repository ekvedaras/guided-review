use guided_review::app_icon::{self, AppIcon, Look};
use guided_review::update::Build;
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
    assert!(brightness(ground(AppIcon::of(Build::Release, Look::Light))) > 200);
}

#[test]
fn the_dark_icon_stands_on_a_dark_ground() {
    assert!(brightness(ground(AppIcon::of(Build::Release, Look::Dark))) < 60);
}

#[test]
fn a_local_build_shows_the_icon_of_the_look_it_is_given() {
    assert_eq!(
        ground(AppIcon::of(Build::Local, Look::Light)),
        ground(AppIcon::of(Build::Release, Look::Light))
    );
    assert_eq!(
        ground(AppIcon::of(Build::Local, Look::Dark)),
        ground(AppIcon::of(Build::Release, Look::Dark))
    );
}

#[test]
fn a_local_build_wears_the_dev_badge_and_the_release_and_the_snapshot_build_do_not() {
    let picture = |build| AppIcon::of(build, Look::Light).png(SIZE).unwrap();
    let release = picture(Build::Release);

    assert_ne!(picture(Build::Local), release);
    assert_eq!(picture(Build::Test), release);
}

#[test]
fn a_window_title_carries_the_badge_of_the_build_that_is_not_the_release() {
    assert_eq!(Build::Release.marked_title("Letters".to_owned()), "Letters");
    assert_eq!(
        Build::Local.marked_title("Letters".to_owned()),
        "Letters (DEV)"
    );
    assert_eq!(
        Build::Test.marked_title("Letters".to_owned()),
        "Letters (TEST)"
    );
}

#[test]
fn the_pictures_are_exported_from_the_icon_composer_icon_as_it_is() {
    assert_eq!(
        app_icon::icon_composer_digest(),
        app_icon::digest_the_pictures_were_exported_from(),
        "AppIcon.icon changed after its pictures were exported: run scripts/app-icon-pictures"
    );
}
