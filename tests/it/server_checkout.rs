use std::path::Path;

use guided_review::codebase::Codebase;
use guided_review::navigation::{Change, ChangedFile, ServerCheckouts};
use guided_review::pull_request::Sha;
use tempfile::TempDir;

use crate::support::{FixtureRepo, write_executable};

struct Rig {
    clone: FixtureRepo,
    state: TempDir,
    checkouts: ServerCheckouts,
    first: Sha,
    second: Sha,
}

fn shop() -> Codebase {
    Codebase::Hosted(crate::support::shop_on_github())
}

impl Rig {
    /// A clone with two commits, left at the second with a dirty working tree.
    fn with_a_dirty_clone() -> Self {
        let clone = FixtureRepo::cloned_from("acme/shop");
        clone.write("src/lib.rs", "pub fn first() {}\n");
        clone.write("README.md", "docs\n");
        let first = Sha::new(&clone.commit("first"));
        clone.write("src/lib.rs", "pub fn second() {}\n");
        clone.write("src/new.rs", "pub fn added() {}\n");
        let second = Sha::new(&clone.commit("second"));
        clone.write("README.md", "edited but not committed\n");
        clone.write("scratch.txt", "untracked\n");
        let state = TempDir::new().unwrap();
        let checkouts = ServerCheckouts::new(state.path().join("server-checkouts"));
        Self {
            clone,
            state,
            checkouts,
            first,
            second,
        }
    }

    fn move_to(&self, head: &Sha) -> guided_review::navigation::Checkout {
        self.checkouts
            .moved_to(self.clone.path(), &shop(), head)
            .unwrap()
    }
}

fn read(folder: &Path, file: &str) -> String {
    std::fs::read_to_string(folder.join(file)).unwrap()
}

#[test]
fn the_first_checkout_is_at_the_reviewed_head_and_leaves_a_dirty_clone_as_it_was() {
    let rig = Rig::with_a_dirty_clone();
    let status_before = rig.clone.git(&["status", "--porcelain"]);
    let index_before = rig.clone.git(&["ls-files", "--stage"]);

    let checkout = rig.move_to(&rig.first);

    assert!(checkout.folder.starts_with(rig.state.path()));
    assert_eq!(read(&checkout.folder, "src/lib.rs"), "pub fn first() {}\n");
    assert_eq!(
        read(rig.clone.path(), "README.md"),
        "edited but not committed\n"
    );
    assert_eq!(read(rig.clone.path(), "src/lib.rs"), "pub fn second() {}\n");
    assert_eq!(rig.clone.git(&["status", "--porcelain"]), status_before);
    assert_eq!(rig.clone.git(&["ls-files", "--stage"]), index_before);
    assert_eq!(rig.clone.git(&["rev-parse", "HEAD"]), rig.second.as_str());
    assert_eq!(rig.clone.git(&["branch", "--show-current"]), "main");
}

#[test]
fn the_checkout_borrows_the_clones_installed_dependencies_as_links() {
    let rig = Rig::with_a_dirty_clone();
    rig.clone
        .write("vendor/laravel/framework.php", "<?php // installed\n");
    rig.clone
        .write("node_modules/left-pad/index.js", "module.exports = 1;\n");

    let checkout = rig.move_to(&rig.first);

    for file in [
        "vendor/laravel/framework.php",
        "node_modules/left-pad/index.js",
    ] {
        assert_eq!(read(&checkout.folder, file), read(rig.clone.path(), file));
    }
    assert!(checkout.folder.join("vendor").is_symlink());
    assert!(checkout.folder.join("node_modules").is_symlink());
    assert!(!rig.clone.path().join("vendor").is_symlink());
}

#[test]
fn a_head_that_tracks_a_dependency_folder_replaces_the_link_with_its_files() {
    let rig = Rig::with_a_dirty_clone();
    rig.clone
        .write("vendor/tracked.php", "<?php // committed\n");
    let third = Sha::new(&rig.clone.commit("vendor is committed"));
    let before = rig.move_to(&rig.first);
    assert!(before.folder.join("vendor").is_symlink());

    let checkout = rig.move_to(&third);

    assert!(!checkout.folder.join("vendor").is_symlink());
    assert_eq!(
        read(&checkout.folder, "vendor/tracked.php"),
        "<?php // committed\n"
    );
}

fn listed(changes: Vec<ChangedFile>) -> Vec<(String, Change)> {
    changes
        .into_iter()
        .map(|file| (file.path.as_str().to_owned(), file.change))
        .collect()
}

fn files(changes: &[(&str, Change)]) -> Vec<(String, Change)> {
    changes
        .iter()
        .map(|(path, change)| ((*path).to_owned(), *change))
        .collect()
}

#[test]
fn a_second_review_moves_the_same_checkout_and_says_which_files_changed_since_a_head() {
    let rig = Rig::with_a_dirty_clone();
    let first = rig.move_to(&rig.first);
    assert_eq!(first.changes_since(&rig.first).unwrap(), []);

    let second = rig.move_to(&rig.second);

    assert_eq!(second.folder, first.folder);
    assert_eq!(read(&second.folder, "src/lib.rs"), "pub fn second() {}\n");
    assert_eq!(
        listed(second.changes_since(&rig.first).unwrap()),
        files(&[
            ("src/lib.rs", Change::Changed),
            ("src/new.rs", Change::Created)
        ])
    );
    let back = rig.move_to(&rig.first);
    assert_eq!(
        listed(back.changes_since(&rig.second).unwrap()),
        files(&[
            ("src/lib.rs", Change::Changed),
            ("src/new.rs", Change::Deleted)
        ])
    );
}

#[test]
fn moving_the_checkout_runs_no_git_hook_from_the_prs_files() {
    let rig = Rig::with_a_dirty_clone();
    let marker = rig.state.path().join("hook-ran");
    rig.clone.git(&["config", "core.hooksPath", "hooks"]);
    std::fs::create_dir(rig.clone.path().join("hooks")).unwrap();
    write_executable(
        &rig.clone.path().join("hooks/post-checkout"),
        format!("#!/bin/sh\ntouch '{}'\n", marker.display()),
    );
    let with_hook = Sha::new(&rig.clone.commit("a hook in the PR"));

    rig.move_to(&with_hook);
    rig.move_to(&rig.first);
    rig.move_to(&with_hook);

    assert!(!marker.exists(), "a hook ran");
}

#[test]
fn moving_the_checkout_records_nothing_in_the_clone() {
    let rig = Rig::with_a_dirty_clone();
    let refs_before = rig.clone.git(&["for-each-ref"]);

    rig.move_to(&rig.first);
    rig.move_to(&rig.second);

    assert!(!rig.clone.path().join(".git/worktrees").exists());
    assert_eq!(
        rig.clone
            .git(&["worktree", "list", "--porcelain"])
            .lines()
            .count(),
        3
    );
    assert_eq!(rig.clone.git(&["for-each-ref"]), refs_before);
}

/// Another clone of the repo, with one more commit than `clone`'s head.
fn another_clone_with_a_commit(clone: &FixtureRepo) -> (TempDir, Sha) {
    let other = TempDir::new().unwrap();
    git_in(
        other.path(),
        &["clone", "-q", &clone.path().to_string_lossy(), "."],
    );
    std::fs::write(other.path().join("src/other.rs"), "pub fn other() {}\n").unwrap();
    git_in(other.path(), &["add", "-A"]);
    git_in(other.path(), &["commit", "-q", "-m", "in the other clone"]);
    let head = Sha::new(&git_in(other.path(), &["rev-parse", "HEAD"]));
    (other, head)
}

fn git_in(folder: &Path, args: &[&str]) -> String {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(folder)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "Fixture")
        .env("GIT_AUTHOR_EMAIL", "fixture@example.com")
        .env("GIT_COMMITTER_NAME", "Fixture")
        .env("GIT_COMMITTER_EMAIL", "fixture@example.com")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

#[test]
fn a_move_from_another_clone_works_after_the_first_clone_is_gone() {
    let rig = Rig::with_a_dirty_clone();
    let (other, head) = another_clone_with_a_commit(&rig.clone);
    rig.clone
        .write("src/only_here.rs", "pub fn only_here() {}\n");
    let only_in_the_first_clone = Sha::new(&rig.clone.commit("only in the first clone"));
    rig.move_to(&only_in_the_first_clone);
    std::fs::remove_dir_all(rig.clone.path()).unwrap();

    let checkout = rig
        .checkouts
        .moved_to(other.path(), &shop(), &head)
        .unwrap();

    assert_eq!(
        read(&checkout.folder, "src/other.rs"),
        "pub fn other() {}\n"
    );
    assert_eq!(read(&checkout.folder, "src/lib.rs"), "pub fn second() {}\n");
    assert!(!checkout.folder.join("src/only_here.rs").exists());
    let moved_back = rig
        .checkouts
        .moved_to(other.path(), &shop(), &rig.first)
        .unwrap();
    assert_eq!(
        read(&moved_back.folder, "src/lib.rs"),
        "pub fn first() {}\n"
    );
}

#[test]
fn links_to_installed_dependencies_point_at_the_folder_the_review_reads() {
    let rig = Rig::with_a_dirty_clone();
    rig.clone.write("vendor/installed.php", "<?php // clone\n");
    rig.clone
        .write("node_modules/left-pad/index.js", "module.exports = 1;\n");
    rig.move_to(&rig.first);
    let agent_folder = TempDir::new().unwrap();
    let agent_worktree = agent_folder.path().join("agent");
    rig.clone.git(&[
        "worktree",
        "add",
        "-q",
        "--detach",
        &agent_worktree.to_string_lossy(),
        rig.second.as_str(),
    ]);
    std::fs::create_dir_all(agent_worktree.join("vendor")).unwrap();
    std::fs::write(
        agent_worktree.join("vendor/installed.php"),
        "<?php // agent\n",
    )
    .unwrap();

    let checkout = rig
        .checkouts
        .moved_to(&agent_worktree, &shop(), &rig.second)
        .unwrap();

    assert_eq!(
        std::fs::read_link(checkout.folder.join("vendor")).unwrap(),
        agent_worktree.join("vendor")
    );
    assert_eq!(
        read(&checkout.folder, "vendor/installed.php"),
        "<?php // agent\n"
    );
    assert!(
        checkout
            .folder
            .join("node_modules")
            .symlink_metadata()
            .is_err()
    );
}

#[test]
fn a_worktree_checkout_an_earlier_build_made_is_replaced_and_its_entry_in_the_clone_removed() {
    let rig = Rig::with_a_dirty_clone();
    rig.clone
        .write("vendor/installed.php", "<?php // installed\n");
    let earlier = rig.state.path().join("server-checkouts/acme/shop");
    std::fs::create_dir_all(earlier.parent().unwrap()).unwrap();
    rig.clone.git(&[
        "worktree",
        "add",
        "-q",
        "--detach",
        &earlier.to_string_lossy(),
        rig.first.as_str(),
    ]);
    std::os::unix::fs::symlink(rig.clone.path().join("vendor"), earlier.join("vendor")).unwrap();

    let checkout = rig.move_to(&rig.second);

    assert_eq!(checkout.folder, earlier);
    assert!(checkout.folder.join(".git").is_dir());
    assert_eq!(read(&checkout.folder, "src/new.rs"), "pub fn added() {}\n");
    assert_eq!(
        rig.clone
            .git(&["worktree", "list", "--porcelain"])
            .lines()
            .count(),
        3
    );
    assert_eq!(
        read(rig.clone.path(), "vendor/installed.php"),
        "<?php // installed\n"
    );
}

#[test]
fn checkouts_and_indexes_earlier_builds_kept_under_local_folder_are_removed() {
    let rig = Rig::with_a_dirty_clone();
    let leftover = rig
        .state
        .path()
        .join("server-checkouts/local folder/shop-1234abcd");
    std::fs::create_dir_all(leftover.parent().unwrap()).unwrap();
    rig.clone.git(&[
        "worktree",
        "add",
        "-q",
        "--detach",
        &leftover.to_string_lossy(),
        rig.first.as_str(),
    ]);
    let indexes = rig.state.path().join("server-indexes");
    std::fs::create_dir_all(indexes.join("local folder/shop-1234abcd")).unwrap();
    std::fs::create_dir_all(indexes.join("acme/shop")).unwrap();

    rig.checkouts
        .remove_local_folder_leftovers(&indexes)
        .unwrap();

    assert!(
        !rig.state
            .path()
            .join("server-checkouts/local folder")
            .exists()
    );
    assert!(!indexes.join("local folder").exists());
    assert!(indexes.join("acme/shop").exists());
    assert_eq!(
        rig.clone
            .git(&["worktree", "list", "--porcelain"])
            .lines()
            .count(),
        3
    );
}

/// Files that make a server, or the cargo or rustup it runs, start a program the PR names.
const PROGRAM_SETTINGS: [&str; 8] = [
    ".cargo/config.toml",
    ".cargo/config",
    "crates/shop/.cargo/config.toml",
    "rust-toolchain.toml",
    "crates/shop/rust-toolchain",
    "rust-project.json",
    "crates/shop/rust-project.json",
    "rust-analyzer.toml",
];

#[test]
fn program_settings_in_the_prs_files_are_kept_out_of_the_checkout_and_stay_in_the_clone() {
    let rig = Rig::with_a_dirty_clone();
    for file in PROGRAM_SETTINGS {
        rig.clone.write(file, "hostile\n");
    }
    let hostile = Sha::new(&rig.clone.commit("program settings"));

    let checkout = rig.move_to(&hostile);

    for file in PROGRAM_SETTINGS {
        assert!(
            !checkout.folder.join(file).exists(),
            "{file} is in the checkout"
        );
        assert!(
            rig.clone.path().join(file).exists(),
            "{file} left the clone"
        );
    }
    assert_eq!(read(&checkout.folder, "src/lib.rs"), "pub fn second() {}\n");
}

#[test]
fn a_cargo_config_an_earlier_build_checked_out_is_removed_by_the_next_move() {
    let rig = Rig::with_a_dirty_clone();
    rig.clone.write(
        ".cargo/config.toml",
        "[build]\nrustc-wrapper = \"wrap.sh\"\n",
    );
    let hostile = Sha::new(&rig.clone.commit("a cargo config"));
    let checkout = rig.move_to(&hostile);
    git_in(&checkout.folder, &["sparse-checkout", "disable"]);
    assert!(checkout.folder.join(".cargo/config.toml").exists());

    let checkout = rig.move_to(&hostile);

    assert!(!checkout.folder.join(".cargo/config.toml").exists());
}
