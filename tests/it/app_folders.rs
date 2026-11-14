use std::ffi::OsString;
use std::fs::{self, File};
use std::path::Path;

use guided_review::app_folders::AppFolders;
use guided_review::platform;
use tempfile::TempDir;

use crate::support::folders_in;

fn env<'a>(values: &'a [(&str, &str)]) -> impl Fn(&str) -> Option<OsString> + 'a {
    |name| {
        values
            .iter()
            .find(|(key, _)| *key == name)
            .map(|(_, value)| OsString::from(value))
    }
}

#[test]
fn with_no_xdg_variables_the_folders_are_the_xdg_defaults_under_home() {
    let folders = platform::app_folders(Path::new("/home/me"), env(&[]));

    assert_eq!(
        folders.config_file(),
        Path::new("/home/me/.config/guided-review/config.toml")
    );
    assert_eq!(
        folders.tours(),
        Path::new("/home/me/.local/state/guided-review/tours")
    );
    assert_eq!(
        folders.images(),
        Path::new("/home/me/.cache/guided-review/images")
    );
    assert_eq!(
        folders.socket(),
        Path::new("/home/me/.local/state/guided-review/app.sock")
    );
}

#[test]
fn xdg_variables_name_the_folders() {
    let folders = platform::app_folders(
        Path::new("/home/me"),
        env(&[
            ("XDG_CONFIG_HOME", "/cfg"),
            ("XDG_STATE_HOME", "/st"),
            ("XDG_CACHE_HOME", "/ca"),
            ("XDG_RUNTIME_DIR", "/run/user/1000"),
        ]),
    );

    assert_eq!(
        folders.config_file(),
        Path::new("/cfg/guided-review/config.toml")
    );
    assert_eq!(
        folders.summary_file(),
        Path::new("/cfg/guided-review/summary.md")
    );
    assert_eq!(folders.tours(), Path::new("/st/guided-review/tours"));
    assert_eq!(folders.state_files(), Path::new("/st/guided-review/state"));
    assert_eq!(folders.evals(), Path::new("/st/guided-review/evals"));
    assert_eq!(folders.lock(), Path::new("/st/guided-review/app.lock"));
    assert_eq!(folders.log(), Path::new("/st/guided-review/app.log"));
    assert_eq!(folders.images(), Path::new("/ca/guided-review/images"));
    assert_eq!(
        folders.socket(),
        Path::new("/run/user/1000/guided-review/app.sock")
    );
}

#[test]
fn an_empty_or_relative_xdg_variable_is_ignored_as_the_xdg_spec_says() {
    let folders = platform::app_folders(
        Path::new("/home/me"),
        env(&[
            ("XDG_CONFIG_HOME", ""),
            ("XDG_STATE_HOME", "relative/state"),
        ]),
    );

    assert_eq!(
        folders.config_file(),
        Path::new("/home/me/.config/guided-review/config.toml")
    );
    assert_eq!(
        folders.tours(),
        Path::new("/home/me/.local/state/guided-review/tours")
    );
}

#[test]
fn the_folders_before_xdg_are_all_in_dot_guided_review() {
    let folders = AppFolders::before_xdg(Path::new("/home/me"));

    assert_eq!(
        folders.config_file(),
        Path::new("/home/me/.guided-review/config.toml")
    );
    assert_eq!(
        folders.state_files(),
        Path::new("/home/me/.guided-review/state")
    );
    assert_eq!(
        folders.images(),
        Path::new("/home/me/.guided-review/cache/images")
    );
    assert_eq!(
        folders.socket(),
        Path::new("/home/me/.guided-review/app.sock")
    );
}

/// What a reviewer of an earlier version has in `~/.guided-review`, by the paths of `folders`.
const WRITTEN: [(&str, &str); 6] = [
    ("config", "ask_model = \"sonnet\"\n"),
    ("summary", "Open with the risk.\n"),
    ("tour", "{\"title\":\"A change\"}"),
    ("state", "{\"read\":[\"a\"]}"),
    ("eval", "a reviewer note"),
    ("log", "started\n"),
];

fn path_of(folders: &AppFolders, name: &str) -> std::path::PathBuf {
    match name {
        "config" => folders.config_file(),
        "summary" => folders.summary_file(),
        "tour" => folders.tours().join("acme/shop/7.json"),
        "state" => folders.state_files().join("acme/shop/7.json"),
        "eval" => folders.evals().join("notes/acme/shop/7.md"),
        _ => folders.log(),
    }
}

fn write_all(folders: &AppFolders) {
    for (name, text) in WRITTEN {
        let path = path_of(folders, name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
}

fn assert_all_in(folders: &AppFolders) {
    for (name, text) in WRITTEN {
        let path = path_of(folders, name);
        assert_eq!(
            fs::read_to_string(&path).ok().as_deref(),
            Some(text),
            "{}",
            path.display()
        );
    }
}

fn old_install() -> (TempDir, AppFolders, AppFolders) {
    let home = TempDir::new().unwrap();
    let old = AppFolders::before_xdg(home.path());
    write_all(&old);
    fs::write(old.lock(), "").unwrap();
    // A cache is not moved: the new one fills again.
    fs::create_dir_all(old.images()).unwrap();
    fs::write(old.images().join("abc123"), "png bytes").unwrap();
    let xdg = folders_in(home.path());
    (home, old, xdg)
}

#[test]
fn the_old_folder_is_moved_into_the_xdg_folders_and_left_behind_empty() {
    let (home, _old, xdg) = old_install();

    assert_eq!(AppFolders::move_once(xdg.clone()).unwrap(), xdg);

    assert_all_in(&xdg);
    assert!(!home.path().join(".guided-review").exists());
    assert_eq!(xdg.old_folder_written_again(), None);
}

#[test]
fn a_second_start_moves_nothing() {
    let (home, _old, xdg) = old_install();
    AppFolders::move_once(xdg.clone()).unwrap();
    fs::write(xdg.config_file(), "changed since\n").unwrap();

    assert_eq!(AppFolders::move_once(xdg.clone()).unwrap(), xdg);

    assert_eq!(
        fs::read_to_string(xdg.config_file()).unwrap(),
        "changed since\n"
    );
    assert!(!home.path().join(".guided-review").exists());
}

#[test]
fn files_of_the_reviewer_s_own_in_the_old_folder_stay_there() {
    let (home, _old, xdg) = old_install();
    let own = home.path().join(".guided-review/elsewhere.md");
    fs::write(&own, "mine").unwrap();

    AppFolders::move_once(xdg.clone()).unwrap();

    assert_all_in(&xdg);
    assert_eq!(fs::read_to_string(own).unwrap(), "mine");
    assert_eq!(xdg.old_folder_written_again(), None);
}

#[test]
fn while_an_old_app_holds_the_lock_nothing_moves_and_the_old_folder_stays_in_use() {
    let (_home, old, xdg) = old_install();
    let running_app = File::options().write(true).open(old.lock()).unwrap();
    running_app.try_lock().unwrap();

    assert_eq!(AppFolders::move_once(xdg.clone()).unwrap(), old);

    assert_all_in(&old);
    assert!(!xdg.config_file().parent().unwrap().exists());
    assert!(!xdg.tours().parent().unwrap().exists());

    drop(running_app);
    assert_eq!(AppFolders::move_once(xdg.clone()).unwrap(), xdg);
    assert_all_in(&xdg);
}

#[test]
fn a_move_that_fails_part_way_puts_everything_back_and_the_old_folder_stays_in_use() {
    let (home, old, xdg) = old_install();
    // A file where the state folder goes: the config moves, then the tours cannot.
    fs::create_dir_all(home.path().join(".local")).unwrap();
    fs::write(home.path().join(".local/state"), "in the way").unwrap();

    assert_eq!(AppFolders::move_once(xdg.clone()).unwrap(), old);

    assert_all_in(&old);
    assert!(!xdg.config_file().parent().unwrap().exists());
    assert!(!xdg.tours().parent().unwrap().exists());

    fs::remove_file(home.path().join(".local/state")).unwrap();
    assert_eq!(AppFolders::move_once(xdg.clone()).unwrap(), xdg);
    assert_all_in(&xdg);
}

#[test]
fn a_move_cut_short_is_finished_by_the_next_start() {
    let (home, old, xdg) = old_install();
    // As a move killed with the config renamed, the summary copied to another disk but not yet
    // deleted, and the tours half copied. The rest is still in the old folder.
    fs::write(home.path().join(".guided-review/.moving"), "").unwrap();
    fs::create_dir_all(xdg.config_file().parent().unwrap()).unwrap();
    fs::rename(old.config_file(), xdg.config_file()).unwrap();
    fs::copy(old.summary_file(), xdg.summary_file()).unwrap();
    let half_copied = xdg.tours().with_extension("partial");
    fs::create_dir_all(half_copied.join("acme")).unwrap();

    assert_eq!(AppFolders::move_once(xdg.clone()).unwrap(), xdg);

    assert_all_in(&xdg);
    assert!(!home.path().join(".guided-review").exists());
    assert!(!xdg.tours().with_extension("partial").exists());
}

#[test]
fn an_old_binary_writing_the_old_folder_after_the_move_is_a_problem_and_nothing_is_merged() {
    let (home, old, xdg) = old_install();
    AppFolders::move_once(xdg.clone()).unwrap();
    let written_again = path_of(&old, "state");
    fs::create_dir_all(written_again.parent().unwrap()).unwrap();
    fs::write(&written_again, "{\"read\":[]}").unwrap();

    assert_eq!(AppFolders::move_once(xdg.clone()).unwrap(), xdg);

    assert_all_in(&xdg);
    assert_eq!(fs::read_to_string(written_again).unwrap(), "{\"read\":[]}");
    assert_eq!(
        xdg.old_folder_written_again(),
        Some(home.path().join(".guided-review"))
    );
}

#[test]
fn a_move_to_another_disk_copies_checks_and_then_deletes() {
    let (home, _old, _) = old_install();
    // Under the build folder, which is often on another disk than the system's temp folder.
    let elsewhere = TempDir::new_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let base = elsewhere.path().display().to_string();
    let xdg = platform::app_folders(
        home.path(),
        env(&[
            ("XDG_CONFIG_HOME", &format!("{base}/config")),
            ("XDG_STATE_HOME", &format!("{base}/state")),
            ("XDG_CACHE_HOME", &format!("{base}/cache")),
        ]),
    );

    assert_eq!(AppFolders::move_once(xdg.clone()).unwrap(), xdg);

    assert_all_in(&xdg);
    assert!(!home.path().join(".guided-review").exists());
}
