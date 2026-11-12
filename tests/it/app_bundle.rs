use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};

use guided_review::update::{
    AppBundle, BundlePlaced, Relaunch, Version, info_plist, relaunch_in_login_shell,
    write_bundle_template,
};
use tempfile::TempDir;

fn version(text: &str) -> Version {
    text.parse().unwrap()
}

/// A `Guided Review.app` as the release workflow packs it: the Info.plist and the icon, no executable.
fn template(dir: &Path, version_text: &str) -> PathBuf {
    let app = dir.join("Guided Review.app");
    fs::create_dir_all(app.join("Contents/Resources")).unwrap();
    fs::write(
        app.join("Contents/Info.plist"),
        info_plist(version(version_text)),
    )
    .unwrap();
    fs::write(
        app.join("Contents/Resources/AppIcon.icns"),
        format!("icon {version_text}"),
    )
    .unwrap();
    app
}

fn home() -> TempDir {
    TempDir::new().unwrap()
}

fn bundle_in(home: &Path) -> PathBuf {
    home.join("Applications/Guided Review.app")
}

#[test]
fn the_info_plist_names_the_app_its_version_its_icon_and_the_binary_it_runs() {
    let plist = info_plist(version("0.7.0"));

    for line in [
        "<key>CFBundleName</key>\n\t<string>Guided Review</string>",
        "<key>CFBundleDisplayName</key>\n\t<string>Guided Review</string>",
        "<key>CFBundleIdentifier</key>\n\t<string>io.github.ekvedaras.guided-review</string>",
        "<key>CFBundleExecutable</key>\n\t<string>guided-review</string>",
        "<key>CFBundleIconFile</key>\n\t<string>AppIcon</string>",
        "<key>CFBundleShortVersionString</key>\n\t<string>0.7.0</string>",
        "<key>CFBundleVersion</key>\n\t<string>0.7.0</string>",
        "<key>CFBundlePackageType</key>\n\t<string>APPL</string>",
        "<key>LSMinimumSystemVersion</key>\n\t<string>11.0</string>",
    ] {
        assert!(plist.contains(line), "{line} is not in\n{plist}");
    }
    assert!(plist.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist"));
}

#[test]
fn the_template_holds_the_info_plist_and_every_picture_iconutil_needs() {
    let out = TempDir::new().unwrap();

    write_bundle_template(out.path(), version("0.7.0")).unwrap();

    assert_eq!(
        fs::read_to_string(out.path().join("Guided Review.app/Contents/Info.plist")).unwrap(),
        info_plist(version("0.7.0"))
    );
    let mut pictures: Vec<String> = fs::read_dir(out.path().join("AppIcon.iconset"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    pictures.sort();
    assert_eq!(
        pictures,
        [
            "icon_128x128.png",
            "icon_128x128@2x.png",
            "icon_16x16.png",
            "icon_16x16@2x.png",
            "icon_256x256.png",
            "icon_256x256@2x.png",
            "icon_32x32.png",
            "icon_32x32@2x.png",
            "icon_512x512.png",
            "icon_512x512@2x.png",
        ]
    );
    let picture = fs::read(out.path().join("AppIcon.iconset/icon_512x512@2x.png")).unwrap();
    assert!(picture.starts_with(b"\x89PNG"));
}

#[test]
fn placing_the_bundle_makes_a_real_app_folder_whose_binary_is_the_bin_link() {
    let home = home();
    let release = TempDir::new().unwrap();
    let template = template(release.path(), "0.7.0");

    let placed = AppBundle::in_home(home.path())
        .place_from(&template)
        .unwrap();

    let bundle = bundle_in(home.path());
    assert_eq!(placed, BundlePlaced::Refreshed);
    assert!(fs::symlink_metadata(&bundle).unwrap().is_dir());
    assert_eq!(
        fs::read_to_string(bundle.join("Contents/Info.plist")).unwrap(),
        info_plist(version("0.7.0"))
    );
    assert_eq!(
        fs::read_to_string(bundle.join("Contents/Resources/AppIcon.icns")).unwrap(),
        "icon 0.7.0"
    );
    assert_eq!(
        fs::read_link(bundle.join("Contents/MacOS/guided-review")).unwrap(),
        home.path().join(".local/bin/guided-review")
    );
}

#[test]
fn placing_the_same_release_again_changes_nothing() {
    let home = home();
    let release = TempDir::new().unwrap();
    let template = template(release.path(), "0.7.0");
    let bundle = AppBundle::in_home(home.path());
    bundle.place_from(&template).unwrap();

    assert_eq!(bundle.place_from(&template).unwrap(), BundlePlaced::Current);
}

#[test]
fn placing_a_newer_release_replaces_the_files_of_the_older_one() {
    let home = home();
    let (old, new) = (TempDir::new().unwrap(), TempDir::new().unwrap());
    let bundle = AppBundle::in_home(home.path());
    bundle.place_from(&template(old.path(), "0.7.0")).unwrap();

    let placed = bundle.place_from(&template(new.path(), "0.8.0")).unwrap();

    assert_eq!(placed, BundlePlaced::Refreshed);
    let app = bundle_in(home.path());
    assert_eq!(
        fs::read_to_string(app.join("Contents/Info.plist")).unwrap(),
        info_plist(version("0.8.0"))
    );
    assert_eq!(
        fs::read_to_string(app.join("Contents/Resources/AppIcon.icns")).unwrap(),
        "icon 0.8.0"
    );
}

#[test]
fn a_release_without_a_bundle_places_none() {
    let home = home();
    let release = TempDir::new().unwrap();

    let placed = AppBundle::in_home(home.path())
        .place_from(&release.path().join("Guided Review.app"))
        .unwrap();

    assert_eq!(placed, BundlePlaced::NotInRelease);
    assert!(!bundle_in(home.path()).exists());
}

const BUNDLED: &str = "/Users/you/Applications/Guided Review.app/Contents/MacOS/guided-review";

#[test]
fn a_launch_of_the_bundle_runs_again_through_the_login_shell_so_gh_claude_and_hc_are_found() {
    let relaunch =
        relaunch_in_login_shell(Path::new(BUNDLED), Some(OsStr::new("/bin/bash")), false);

    assert_eq!(
        relaunch,
        Some(Relaunch {
            program: PathBuf::from("/bin/bash"),
            args: ["-l", "-i", "-c", "exec \"$0\"", BUNDLED]
                .map(OsString::from)
                .to_vec(),
        })
    );
}

#[test]
fn with_no_shell_named_the_launch_runs_through_zsh() {
    let relaunch = relaunch_in_login_shell(Path::new(BUNDLED), None, false).unwrap();

    assert_eq!(relaunch.program, PathBuf::from("/bin/zsh"));
}

#[test]
fn a_launch_already_in_the_login_shell_runs_as_it_is() {
    assert_eq!(
        relaunch_in_login_shell(Path::new(BUNDLED), Some(OsStr::new("/bin/zsh")), true),
        None
    );
}

#[test]
fn a_binary_outside_a_bundle_runs_as_it_is() {
    assert_eq!(
        relaunch_in_login_shell(
            Path::new("/Users/you/.local/bin/guided-review"),
            Some(OsStr::new("/bin/zsh")),
            false
        ),
        None
    );
}

fn with_bin_link_to(home: &Path, binary: &str) -> PathBuf {
    let release = home.join(".local/share/guided-review/0.7.0/guided-review");
    fs::create_dir_all(release.parent().unwrap()).unwrap();
    fs::write(&release, binary).unwrap();
    fs::create_dir_all(home.join(".local/bin")).unwrap();
    std::os::unix::fs::symlink(&release, home.join(".local/bin/guided-review")).unwrap();
    release
}

#[test]
fn the_app_is_started_through_the_bundle_when_the_bundle_runs_the_same_binary() {
    let home = home();
    let binary = with_bin_link_to(home.path(), "binary 0.7.0");
    let release = TempDir::new().unwrap();
    let bundle = AppBundle::in_home(home.path());
    bundle
        .place_from(&template(release.path(), "0.7.0"))
        .unwrap();

    assert_eq!(bundle.launcher_for(&binary), bundle.executable());
}

#[test]
fn another_binary_is_started_as_itself() {
    let home = home();
    with_bin_link_to(home.path(), "binary 0.7.0");
    let release = TempDir::new().unwrap();
    let bundle = AppBundle::in_home(home.path());
    bundle
        .place_from(&template(release.path(), "0.7.0"))
        .unwrap();
    let cargo_bin = home.path().join(".cargo/bin/guided-review");
    fs::create_dir_all(cargo_bin.parent().unwrap()).unwrap();
    fs::write(&cargo_bin, "binary from source").unwrap();

    assert_eq!(bundle.launcher_for(&cargo_bin), cargo_bin);
}

#[test]
fn with_no_bundle_the_binary_is_started_as_itself() {
    let home = home();
    let binary = with_bin_link_to(home.path(), "binary 0.7.0");

    assert_eq!(
        AppBundle::in_home(home.path()).launcher_for(&binary),
        binary
    );
}
