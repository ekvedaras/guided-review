use std::path::Path;

use guided_review::navigation::{Change, ServerCheckouts};
use guided_review::pull_request::{RepoName, Sha};
use tempfile::TempDir;

use crate::support::FixtureRepo;

struct Rig {
    clone: FixtureRepo,
    state: TempDir,
    checkouts: ServerCheckouts,
    first: Sha,
    second: Sha,
}

fn shop() -> RepoName {
    RepoName::new("acme", "shop")
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

#[test]
fn a_second_review_moves_the_same_checkout_and_says_which_files_changed() {
    let rig = Rig::with_a_dirty_clone();
    let first = rig.move_to(&rig.first);
    assert_eq!(first.changed, []);

    let second = rig.move_to(&rig.second);

    assert_eq!(second.folder, first.folder);
    assert_eq!(read(&second.folder, "src/lib.rs"), "pub fn second() {}\n");
    let changed: Vec<(&str, Change)> = second
        .changed
        .iter()
        .map(|file| (file.path.as_str(), file.change))
        .collect();
    assert_eq!(
        changed,
        [
            ("src/lib.rs", Change::Changed),
            ("src/new.rs", Change::Created)
        ]
    );
    let back = rig.move_to(&rig.first);
    let changed: Vec<(&str, Change)> = back
        .changed
        .iter()
        .map(|file| (file.path.as_str(), file.change))
        .collect();
    assert_eq!(
        changed,
        [
            ("src/lib.rs", Change::Changed),
            ("src/new.rs", Change::Deleted)
        ]
    );
    assert_eq!(rig.move_to(&rig.first).changed, []);
}
