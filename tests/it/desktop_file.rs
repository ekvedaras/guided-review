//! The Desktop entry on Linux: a desktop file and its icons, written to a temp home on any host.

use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use guided_review::platform::{
    DesktopEntry, EntryPlaced, Os, Relaunch, write_desktop_entry_templates,
};
use guided_review::version::Version;
use tempfile::TempDir;

const OUR_APP: &str = "io.github.ekvedaras.guided-review";
const ICON_SIZES: [&str; 2] = ["256x256", "512x512"];

fn entry(home: &Path) -> Arc<dyn DesktopEntry> {
    Os::Linux.desktop_entry(home, &home.join(".local/bin/guided-review"))
}

fn icon(size: &str) -> String {
    format!("icons/hicolor/{size}/apps/{OUR_APP}.png")
}

/// A release folder with the Linux entry's icons, as the release workflow packs them.
fn release_with_icons(dir: &Path, content: &str) -> PathBuf {
    for size in ICON_SIZES {
        let path = dir.join(icon(size));
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, format!("{content} {size}")).unwrap();
    }
    dir.to_path_buf()
}

fn desktop_file_in(home: &Path) -> PathBuf {
    home.join(format!(".local/share/applications/{OUR_APP}.desktop"))
}

#[test]
fn the_template_holds_the_icon_pictures_the_linux_entry_names() {
    let out = TempDir::new().unwrap();

    write_desktop_entry_templates(out.path(), "0.7.0".parse::<Version>().unwrap()).unwrap();

    for size in ICON_SIZES {
        let picture = fs::read(out.path().join(icon(size))).unwrap();
        assert!(picture.starts_with(b"\x89PNG"), "{size} is no PNG");
    }
}

#[test]
fn placing_the_entry_writes_a_desktop_file_that_starts_the_bin_link_marked_as_a_launch() {
    let home = TempDir::new().unwrap();
    let release = TempDir::new().unwrap();

    let placed = entry(home.path())
        .place_from(&release_with_icons(release.path(), "icon 0.7.0"))
        .unwrap();

    assert_eq!(placed, EntryPlaced::Refreshed);
    let bin_link = home.path().join(".local/bin/guided-review");
    assert_eq!(
        fs::read_to_string(desktop_file_in(home.path())).unwrap(),
        format!(
            "[Desktop Entry]\n\
             Type=Application\n\
             Name=Guided Review\n\
             Comment=Turn a PR diff into a top-down guided review\n\
             Exec=env GUIDED_REVIEW_LAUNCHED_APP={OUR_APP} \"{}\"\n\
             Icon={OUR_APP}\n\
             Terminal=false\n\
             Categories=Development;\n\
             StartupWMClass={OUR_APP}\n",
            bin_link.display()
        )
    );
    for size in ICON_SIZES {
        assert_eq!(
            fs::read_to_string(home.path().join(".local/share").join(icon(size))).unwrap(),
            format!("icon 0.7.0 {size}")
        );
    }
}

#[test]
fn the_exec_line_escapes_what_a_desktop_file_reads_as_special_in_the_home_path() {
    let parent = TempDir::new().unwrap();
    let home = parent.path().join("we \"two\" $have 100% \\ space");
    let release = TempDir::new().unwrap();

    entry(&home)
        .place_from(&release_with_icons(release.path(), "icon"))
        .unwrap();

    let file = fs::read_to_string(desktop_file_in(&home)).unwrap();
    let exec = file.lines().find(|line| line.starts_with("Exec=")).unwrap();
    assert_eq!(
        exec,
        format!(
            "Exec=env GUIDED_REVIEW_LAUNCHED_APP={OUR_APP} \"{}/we \\\\\"two\\\\\" \\\\$have 100%% \\\\\\\\ space/.local/bin/guided-review\"",
            parent.path().display()
        )
    );
}

#[test]
fn placing_the_same_release_again_changes_nothing() {
    let home = TempDir::new().unwrap();
    let release = TempDir::new().unwrap();
    let release = release_with_icons(release.path(), "icon 0.7.0");
    entry(home.path()).place_from(&release).unwrap();

    assert_eq!(
        entry(home.path()).place_from(&release).unwrap(),
        EntryPlaced::Current
    );
}

#[test]
fn placing_a_newer_release_replaces_the_icons_of_the_older_one() {
    let home = TempDir::new().unwrap();
    let (old, new) = (TempDir::new().unwrap(), TempDir::new().unwrap());
    entry(home.path())
        .place_from(&release_with_icons(old.path(), "icon 0.7.0"))
        .unwrap();

    let placed = entry(home.path())
        .place_from(&release_with_icons(new.path(), "icon 0.8.0"))
        .unwrap();

    assert_eq!(placed, EntryPlaced::Refreshed);
    assert_eq!(
        fs::read_to_string(home.path().join(".local/share").join(icon("256x256"))).unwrap(),
        "icon 0.8.0 256x256"
    );
}

#[test]
fn a_release_without_the_linux_icons_places_no_entry() {
    let home = TempDir::new().unwrap();
    let release = TempDir::new().unwrap();

    let placed = entry(home.path()).place_from(release.path()).unwrap();

    assert_eq!(placed, EntryPlaced::NotInRelease);
    assert!(!desktop_file_in(home.path()).exists());
}

#[test]
fn a_launch_from_the_desktop_file_runs_the_bin_link_again_through_the_login_shell() {
    let entry = entry(Path::new("/home/you"));
    let launched_app = OsString::from(OUR_APP);

    let relaunch = entry.relaunch_in_login_shell(
        Some(&launched_app),
        Some(OsStr::new("/usr/bin/fish")),
        false,
    );

    assert_eq!(entry.launched_app_variable(), "GUIDED_REVIEW_LAUNCHED_APP");
    assert_eq!(
        relaunch,
        Some(Relaunch {
            program: PathBuf::from("/usr/bin/fish"),
            args: [
                "-l",
                "-i",
                "-c",
                "exec \"$0\"",
                "/home/you/.local/bin/guided-review",
            ]
            .map(OsString::from)
            .to_vec(),
        })
    );
}

#[test]
fn with_no_shell_named_the_launch_runs_through_bash() {
    let relaunch = entry(Path::new("/home/you"))
        .relaunch_in_login_shell(Some(OsStr::new(OUR_APP)), None, false)
        .unwrap();

    assert_eq!(relaunch.program, PathBuf::from("/bin/bash"));
}

#[test]
fn a_run_not_launched_from_the_desktop_file_runs_as_it_is() {
    let entry = entry(Path::new("/home/you"));

    assert_eq!(
        entry.relaunch_in_login_shell(None, Some(OsStr::new("/bin/bash")), false),
        None
    );
    assert_eq!(
        entry.relaunch_in_login_shell(
            Some(OsStr::new(OUR_APP)),
            Some(OsStr::new("/bin/bash")),
            true
        ),
        None
    );
}

#[test]
fn the_app_is_started_as_the_binary_itself() {
    let exe = Path::new("/home/you/.local/share/guided-review/0.7.0/guided-review");

    assert_eq!(entry(Path::new("/home/you")).start_program_for(exe), exe);
}
