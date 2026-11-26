use std::collections::BTreeSet;
use std::path::Path;

use guided_review::app_folders::AppFolders;
use guided_review::code_source::CodeSource;
use guided_review::local_facts::LocalChange;
use guided_review::local_git::LocalGit;
use guided_review::local_work_tree::{LocalWorkTree, OpenRefusal, snapshot_ref};
use guided_review::opened_subject::OpenedChangeSet;
use guided_review::pull_request::{HostSite, HostedRepo, RepoName, Sha};
use guided_review::review_subject::{LocalChangeSet, ReviewSubject};

use crate::support::{FixtureRepo, file_path};

/// `main` with a README and a `.gitignore`, then `feature` one commit ahead, with a modified tracked
/// file, an untracked file and an ignored one on disk.
fn shop_with_work_in_progress() -> FixtureRepo {
    let repo = FixtureRepo::cloned_from("acme/shop");
    repo.write("README.md", "hello\n");
    repo.write(".gitignore", "target/\n");
    repo.commit("Start the shop");
    repo.git(&["checkout", "-q", "-b", "feature"]);
    repo.write("src/cart.rs", "pub fn total() {}\n");
    repo.commit("Add the cart");
    repo.write("README.md", "hello\nworld\n");
    repo.write("notes.txt", "draft\n");
    repo.write("target/out.bin", "built\n");
    repo
}

fn work_tree(repo: &FixtureRepo) -> LocalWorkTree {
    LocalWorkTree::containing(repo.path()).unwrap()
}

fn snapshot_of(repo: &FixtureRepo) -> (LocalWorkTree, Sha) {
    let work_tree = work_tree(repo);
    let snapshot = work_tree.make_snapshot().unwrap();
    (work_tree, snapshot.head)
}

/// Only an opened change set moves a snapshot ref, under the lock that keeps it on the tour's snapshot.
/// A snapshot of the same files over the same `HEAD` is the same commit, so `make_snapshot` names the
/// one the ref keeps.
fn open_review_of(folders: &AppFolders, work_tree: &LocalWorkTree) -> (LocalChangeSet, Sha) {
    let change_set = work_tree.change_set().unwrap();
    OpenedChangeSet::new(&change_set, folders)
        .make_basic_tour_if_no_tour()
        .unwrap();
    (change_set, work_tree.make_snapshot().unwrap().head)
}

fn files_of(repo: &FixtureRepo, commit: &Sha) -> BTreeSet<String> {
    repo.git(&["ls-tree", "-r", "--name-only", commit.as_str()])
        .lines()
        .map(str::to_owned)
        .collect()
}

/// Everything of the reviewer's that a snapshot must leave as it was.
fn reviewers_state(repo: &FixtureRepo) -> Vec<String> {
    vec![
        repo.git(&["status", "--porcelain", "--untracked-files=all"]),
        repo.git(&["stash", "list"]),
        repo.git(&["rev-parse", "HEAD"]),
        repo.git(&["branch", "--list"]),
        repo.git(&["diff", "--cached"]),
        repo.git(&["ls-files", "--stage"]),
        repo.git(&["diff"]),
        std::fs::read_to_string(repo.path().join("README.md")).unwrap(),
        std::fs::read_to_string(repo.path().join("notes.txt")).unwrap(),
    ]
}

fn refusal(result: Result<impl std::fmt::Debug, OpenRefusal>) -> String {
    result.unwrap_err().to_string()
}

#[test]
fn a_folder_inside_a_clone_resolves_to_the_clones_root_and_its_current_branch() {
    let repo = shop_with_work_in_progress();
    repo.write("src/deep/mod.rs", "");
    let subfolder = repo.path().join("src/deep");

    let work_tree = LocalWorkTree::containing(&subfolder).unwrap();

    let change_set = work_tree.change_set().unwrap();
    assert_eq!(
        change_set.folder().as_path(),
        repo.path().canonicalize().unwrap()
    );
    assert_eq!(change_set.branch().as_str(), "feature");
    assert_eq!(change_set.repo().unwrap().to_string(), "acme/shop");
}

#[test]
fn a_change_set_keeps_the_code_host_its_origin_is_on() {
    let repo = shop_with_work_in_progress();

    let change_set = work_tree(&repo).change_set().unwrap();

    assert_eq!(
        change_set.origin(),
        Some(&HostedRepo {
            host: HostSite::GitHub,
            repo: RepoName::new("acme", "shop"),
        })
    );
}

#[test]
fn a_clone_whose_origin_is_on_a_host_the_app_does_not_read_names_no_origin() {
    let repo = shop_with_work_in_progress();
    repo.git(&[
        "remote",
        "set-url",
        "origin",
        "git@gitlab.com:acme/shop.git",
    ]);

    let change_set = work_tree(&repo).change_set().unwrap();

    assert_eq!(change_set.origin(), None);
}

#[test]
fn a_folder_that_is_no_clone_is_refused() {
    let folder = tempfile::TempDir::new().unwrap();

    let reason = refusal(LocalWorkTree::containing(folder.path()));

    assert!(reason.contains("not inside a git clone"), "{reason}");
}

#[test]
fn a_linked_git_worktree_is_its_own_working_tree_on_its_own_branch() {
    let repo = shop_with_work_in_progress();
    let linked = tempfile::TempDir::new().unwrap();
    let linked_folder = linked.path().join("shop-fixes");
    repo.git(&[
        "worktree",
        "add",
        "-q",
        "-b",
        "fixes",
        linked_folder.to_str().unwrap(),
        "main",
    ]);

    let in_main = work_tree(&repo).change_set().unwrap();
    let in_linked = LocalWorkTree::containing(&linked_folder.join("."))
        .unwrap()
        .change_set()
        .unwrap();

    assert_eq!(in_linked.branch().as_str(), "fixes");
    assert_eq!(
        in_linked.folder().as_path(),
        linked_folder.canonicalize().unwrap()
    );
    assert_ne!(
        ReviewSubject::from(in_main).to_string(),
        ReviewSubject::from(in_linked).to_string()
    );
}

#[test]
fn a_detached_head_is_refused_with_the_reason() {
    let repo = shop_with_work_in_progress();
    repo.git(&["checkout", "-q", "--detach"]);

    let reason = refusal(work_tree(&repo).change_set());

    assert!(reason.contains("detached HEAD"), "{reason}");
}

#[test]
fn the_default_branch_is_origin_head_when_the_clone_has_one() {
    let repo = shop_with_work_in_progress();
    let upstream_tip = repo.git(&["rev-parse", "main~0"]);
    repo.git(&["update-ref", "refs/remotes/origin/trunk", &upstream_tip]);
    repo.git(&[
        "symbolic-ref",
        "refs/remotes/origin/HEAD",
        "refs/remotes/origin/trunk",
    ]);

    let default = work_tree(&repo).default_branch().unwrap();

    assert_eq!(default.name.as_str(), "origin/trunk");
    assert_eq!(default.tip.as_str(), upstream_tip);
}

#[test]
fn without_origin_head_the_default_branch_is_main_then_master() {
    let repo = shop_with_work_in_progress();
    let main_tip = repo.git(&["rev-parse", "main"]);

    let with_main = work_tree(&repo).default_branch().unwrap();
    repo.git(&["branch", "-m", "main", "master"]);
    let with_master = work_tree(&repo).default_branch().unwrap();

    assert_eq!(with_main.name.as_str(), "main");
    assert_eq!(with_master.name.as_str(), "master");
    assert_eq!(with_master.tip.as_str(), main_tip);
}

#[test]
fn with_no_default_branch_the_refusal_names_the_branches_it_looked_for() {
    let repo = shop_with_work_in_progress();
    repo.git(&["branch", "-m", "main", "develop"]);

    let reason = refusal(work_tree(&repo).default_branch());

    for looked_for in ["refs/remotes/origin/HEAD", "main", "master"] {
        assert!(reason.contains(looked_for), "{reason}");
    }
}

#[test]
fn a_snapshot_holds_commits_tracked_changes_and_untracked_files_but_not_ignored_ones() {
    let repo = shop_with_work_in_progress();
    let head = Sha::new(&repo.git(&["rev-parse", "HEAD"]));

    let (_, snapshot) = snapshot_of(&repo);

    assert_ne!(snapshot, head);
    assert_eq!(
        repo.git(&["rev-parse", &format!("{snapshot}^")]),
        head.as_str()
    );
    assert_eq!(
        files_of(&repo, &snapshot),
        BTreeSet::from([".gitignore", "README.md", "notes.txt", "src/cart.rs"].map(str::to_owned))
    );
    assert_eq!(
        repo.git(&["show", &format!("{snapshot}:README.md")]),
        "hello\nworld"
    );
}

#[test]
fn a_snapshot_of_a_clean_working_tree_is_head_itself() {
    let repo = shop_with_work_in_progress();
    repo.commit("Everything");

    let (_, snapshot) = snapshot_of(&repo);

    assert_eq!(snapshot.as_str(), repo.git(&["rev-parse", "HEAD"]));
}

#[test]
fn a_snapshot_is_kept_by_a_ref_under_guided_review_whose_name_is_never_a_pr_number() {
    let repo = shop_with_work_in_progress();

    let (change_set, snapshot) = open_review_of(&repo.folders(), &work_tree(&repo));

    let name = snapshot_ref(&change_set);
    let leaf = name.strip_prefix("refs/guided-review/").unwrap();
    assert!(!leaf.chars().all(|c| c.is_ascii_digit()), "{name}");
    assert_eq!(repo.git(&["rev-parse", &name]), snapshot.as_str());
    assert!(
        !repo
            .git(&["for-each-ref", "refs/heads", "refs/tags"])
            .contains(&name)
    );
}

#[test]
fn a_snapshot_moves_its_ref_and_leaves_one_ref_per_change_set() {
    let repo = shop_with_work_in_progress();
    let work_tree = work_tree(&repo);
    let (change_set, first) = open_review_of(&repo.folders(), &work_tree);
    repo.write("notes.txt", "more drafts\n");

    OpenedChangeSet::new(&change_set, &repo.folders())
        .repin_to_new_snapshot()
        .unwrap();

    let second = work_tree.make_snapshot().unwrap().head;
    assert_ne!(first, second);
    let refs = repo.git(&[
        "for-each-ref",
        "--format=%(objectname)",
        "refs/guided-review",
    ]);
    assert_eq!(refs, second.as_str());
}

#[test]
fn making_a_snapshot_touches_nothing_of_the_reviewers() {
    let repo = shop_with_work_in_progress();
    repo.git(&["add", "notes.txt"]);
    repo.write("README.md", "stashed\n");
    repo.git(&["stash", "push", "-q", "README.md"]);
    repo.write("README.md", "hello\nworld\n");
    let before = reviewers_state(&repo);

    open_review_of(&repo.folders(), &work_tree(&repo));

    assert_eq!(reviewers_state(&repo), before);
    assert_ne!(before[1], "");
}

#[test]
fn a_branch_with_commits_ahead_has_something_to_review_even_when_clean() {
    let repo = shop_with_work_in_progress();
    repo.commit("Everything");
    let work_tree = work_tree(&repo);
    let change_set = work_tree.change_set().unwrap();

    assert!(work_tree.refuse_when_nothing_to_review(&change_set).is_ok());
}

#[test]
fn uncommitted_files_alone_are_something_to_review() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    repo.write("README.md", "hello\n");
    repo.commit("Start the shop");
    repo.write("notes.txt", "draft\n");
    let work_tree = work_tree(&repo);
    let change_set = work_tree.change_set().unwrap();

    assert!(work_tree.refuse_when_nothing_to_review(&change_set).is_ok());
}

#[test]
fn a_clean_branch_with_no_commits_beyond_the_default_branch_is_refused_with_the_reason() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    repo.write("README.md", "hello\n");
    repo.write(".gitignore", "target/\n");
    repo.commit("Start the shop");
    repo.git(&["checkout", "-q", "-b", "feature"]);
    repo.write("target/out.bin", "built\n");
    let work_tree = work_tree(&repo);
    let change_set = work_tree.change_set().unwrap();

    let reason = refusal(work_tree.refuse_when_nothing_to_review(&change_set));

    assert!(reason.contains("nothing to review"), "{reason}");
    assert!(
        reason.contains("feature") && reason.contains("main"),
        "{reason}"
    );
}

#[test]
fn the_tips_of_a_change_set_are_the_default_branch_and_the_snapshot() {
    let repo = shop_with_work_in_progress();
    let main_tip = repo.git(&["rev-parse", "main"]);
    let (work_tree, snapshot) = snapshot_of(&repo);

    let tips = work_tree
        .tips(&guided_review::local_work_tree::Snapshot {
            head: snapshot.clone(),
        })
        .unwrap();

    assert_eq!(tips.base_branch_tip.as_str(), main_tip);
    assert_eq!(tips.head, snapshot);
}

#[test]
fn the_facts_list_the_commits_and_mark_each_file_committed_uncommitted_or_both() {
    let repo = shop_with_work_in_progress();
    repo.write("src/cart.rs", "pub fn total() -> u32 { 0 }\n");
    repo.write("src/pay.rs", "");
    repo.git(&["add", "src"]);
    repo.git(&["commit", "-q", "-m", "Add payment"]);
    repo.write("src/cart.rs", "pub fn total() -> u32 { 1 }\n");
    let work_tree = work_tree(&repo);
    let change_set = work_tree.change_set().unwrap();
    let snapshot = work_tree.make_snapshot().unwrap();
    let tips = work_tree.tips(&snapshot).unwrap();

    let facts = work_tree.facts(&change_set, &tips).unwrap();

    assert_eq!(facts.commit_subjects, ["Add the cart", "Add payment"]);
    assert_eq!(facts.default_branch.as_str(), "main");
    assert_eq!(facts.branch.as_str(), "feature");
    assert_eq!(facts.origin.as_ref().unwrap().repo.to_string(), "acme/shop");
    assert_eq!(facts.folder.as_path(), repo.path().canonicalize().unwrap());
    assert_eq!(
        facts.change_of(&file_path("src/pay.rs")),
        Some(LocalChange::Committed)
    );
    assert_eq!(
        facts.change_of(&file_path("src/cart.rs")),
        Some(LocalChange::Both)
    );
    assert_eq!(
        facts.change_of(&file_path("README.md")),
        Some(LocalChange::Uncommitted)
    );
    assert_eq!(
        facts.change_of(&file_path("notes.txt")),
        Some(LocalChange::Uncommitted)
    );
    assert_eq!(facts.change_of(&file_path("target/out.bin")), None);
}

#[test]
fn the_facts_of_a_snapshot_stay_as_they_were_when_the_branch_moves_on() {
    let repo = shop_with_work_in_progress();
    let work_tree = work_tree(&repo);
    let change_set = work_tree.change_set().unwrap();
    let snapshot = work_tree.make_snapshot().unwrap();
    let tips = work_tree.tips(&snapshot).unwrap();
    let before = work_tree.facts(&change_set, &tips).unwrap();
    repo.commit("Commit it all");

    let after = work_tree.facts(&change_set, &tips).unwrap();

    assert_eq!(after, before);
}

#[test]
fn the_working_tree_matches_a_snapshot_until_a_file_changes() {
    let repo = shop_with_work_in_progress();
    let (work_tree, snapshot) = snapshot_of(&repo);
    let matched_at_first = work_tree.working_tree_matches(&snapshot);

    repo.write("notes.txt", "different\n");
    let matched_after_an_edit = work_tree.working_tree_matches(&snapshot);
    repo.write("notes.txt", "draft\n");
    let matched_after_the_undo = work_tree.working_tree_matches(&snapshot);
    repo.write("brand-new.txt", "x\n");
    let matched_after_a_new_file = work_tree.working_tree_matches(&snapshot);

    assert!(matched_at_first);
    assert!(!matched_after_an_edit);
    assert!(matched_after_the_undo);
    assert!(!matched_after_a_new_file);
}

#[test]
fn a_commit_of_the_uncommitted_files_still_matches_the_snapshot() {
    let repo = shop_with_work_in_progress();
    let (work_tree, snapshot) = snapshot_of(&repo);

    repo.commit("Commit it all");

    assert!(work_tree.working_tree_matches(&snapshot));
}

#[test]
fn a_code_source_at_the_work_tree_reads_the_snapshot_and_knows_when_the_files_moved() {
    let repo = shop_with_work_in_progress();
    let (work_tree, snapshot) = snapshot_of(&repo);
    let source = LocalGit::at_worktree(work_tree.root());

    let read = source.file_at(&snapshot, &file_path("notes.txt")).unwrap();
    let at_first = source.working_tree_is_at(&snapshot);
    repo.write("notes.txt", "different\n");
    let after_an_edit = source.working_tree_is_at(&snapshot);

    assert_eq!(read, "draft\n");
    assert!(at_first);
    assert!(!after_an_edit);
    assert_eq!(
        source.local_clone(),
        Some(Path::new(work_tree.root().as_path()))
    );
}

#[test]
fn two_worktrees_on_different_branches_are_two_change_sets_with_their_own_snapshot_refs() {
    let repo = shop_with_work_in_progress();
    let linked = tempfile::TempDir::new().unwrap();
    let linked_folder = linked.path().join("shop-fixes");
    repo.git(&[
        "worktree",
        "add",
        "-q",
        "-b",
        "fixes",
        linked_folder.to_str().unwrap(),
        "main",
    ]);
    std::fs::write(linked_folder.join("fix.txt"), "fix\n").unwrap();

    let (first_set, first_snapshot) = open_review_of(&repo.folders(), &work_tree(&repo));
    let (second_set, second_snapshot) = open_review_of(
        &repo.folders(),
        &LocalWorkTree::containing(&linked_folder).unwrap(),
    );

    assert_ne!(snapshot_ref(&first_set), snapshot_ref(&second_set));
    assert_eq!(
        repo.git(&["rev-parse", &snapshot_ref(&first_set)]),
        first_snapshot.as_str()
    );
    assert_eq!(
        repo.git(&["rev-parse", &snapshot_ref(&second_set)]),
        second_snapshot.as_str()
    );
    assert!(files_of(&repo, &second_snapshot).contains("fix.txt"));
}

#[test]
fn a_glance_counts_every_uncommitted_file_once_and_none_that_git_ignores() {
    let repo = shop_with_work_in_progress();
    repo.write("src/old.rs", "gone soon\n");
    repo.write("src/moved.rs", "moves\n");
    repo.commit("Add files that change below");
    repo.git(&["rm", "-q", "src/old.rs"]);
    repo.git(&["mv", "src/moved.rs", "src/renamed.rs"]);
    repo.write("staged.txt", "staged\n");
    repo.git(&["add", "staged.txt"]);
    repo.write("staged.txt", "staged, then changed\n");
    repo.write("deep/new/file.txt", "new\n");
    repo.write("target/more.bin", "ignored\n");

    let glance = work_tree(&repo).glance().unwrap();

    // old (deleted), moved and renamed (two, as renames are not followed), staged, deep/new/file.
    assert_eq!(glance.uncommitted_files, 5);
}
