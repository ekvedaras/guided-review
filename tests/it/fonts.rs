use std::path::PathBuf;

use guided_review::app::components::palette::{CODE_FONT, TEXT_FONT, font_files};
use guided_review::platform::Os;

#[test]
fn macos_names_its_own_fonts_without_asking_what_is_installed() {
    let not_asked = |family: &str| panic!("asked whether {family} is installed");

    assert_eq!(Os::MacOs.text_font(not_asked), "Helvetica Neue");
    assert_eq!(Os::MacOs.code_font(not_asked), "Menlo");
}

#[test]
fn linux_takes_its_first_choice_that_is_installed() {
    assert_eq!(
        Os::Linux.text_font(|family| ["Noto Sans", "Liberation Sans"].contains(&family)),
        "Noto Sans"
    );
    assert_eq!(
        Os::Linux.code_font(|family| family == "Liberation Mono"),
        "Liberation Mono"
    );
}

#[test]
fn linux_falls_back_to_dejavu_when_none_of_its_choices_is_installed() {
    assert_eq!(Os::Linux.text_font(|_| false), "DejaVu Sans");
    assert_eq!(Os::Linux.code_font(|_| false), "DejaVu Sans Mono");
}

#[test]
fn the_app_fonts_are_installed_files() {
    for family in [*TEXT_FONT, *CODE_FONT] {
        let files = font_files(family);

        assert!(!files.is_empty(), "{family} has no file");
        assert!(files.iter().all(|file| file.is_file()), "{files:?}");
    }
}

#[test]
fn a_family_that_is_not_installed_has_no_files() {
    assert_eq!(font_files("No Such Family Anywhere"), Vec::<PathBuf>::new());
}
