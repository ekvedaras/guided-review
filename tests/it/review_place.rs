use std::path::Path;

use guided_review::git_checkout::GitCheckouts;
use guided_review::pull_request::Sha;
use guided_review::review_subject::ReviewSubject;
use guided_review::viewer::{CheckoutPort, ReviewPlace};

use crate::support::FixtureRepo;
use crate::support::fake_checkouts::FakeCheckouts;

fn pr() -> ReviewSubject {
    "https://github.com/acme/shop/pull/7".parse().unwrap()
}

fn place(agent_folder: Option<&str>, checkouts: &FakeCheckouts) -> anyhow::Result<ReviewPlace> {
    ReviewPlace::choose(
        &pr(),
        agent_folder.map(Path::new),
        Path::new("/clones/shop"),
        &Sha::new("abc123"),
        checkouts,
    )
}

fn own_worktree() -> ReviewPlace {
    ReviewPlace::OwnWorktree {
        clone: "/clones/shop".into(),
    }
}

fn the_clone() -> FakeCheckouts {
    FakeCheckouts::default().clean_at("/clones/shop", "other")
}

#[test]
fn a_clean_tour_agent_folder_at_the_reviewed_head_is_where_the_agent_works() {
    let checkouts = the_clone().clean_at("/work/shop", "abc123");

    assert_eq!(
        place(Some("/work/shop"), &checkouts).unwrap(),
        ReviewPlace::TourAgentFolder("/work/shop".into())
    );
}

#[test]
fn a_tour_agent_folder_on_another_commit_sends_the_agent_to_its_own_worktree() {
    let checkouts = the_clone().clean_at("/work/shop", "def456");

    assert_eq!(
        place(Some("/work/shop"), &checkouts).unwrap(),
        own_worktree()
    );
}

#[test]
fn a_dirty_tour_agent_folder_sends_the_agent_to_its_own_worktree() {
    let checkouts = the_clone().dirty_at("/work/shop", "abc123");

    assert_eq!(
        place(Some("/work/shop"), &checkouts).unwrap(),
        own_worktree()
    );
}

#[test]
fn no_tour_agent_sends_the_agent_to_its_own_worktree() {
    assert_eq!(place(None, &the_clone()).unwrap(), own_worktree());
}

#[test]
fn a_tour_agent_folder_that_is_no_checkout_sends_the_agent_to_its_own_worktree() {
    assert_eq!(
        place(Some("/work/notes"), &the_clone()).unwrap(),
        own_worktree()
    );
}

#[test]
fn a_pr_with_no_clone_fails_with_what_to_do_not_with_gits_message() {
    let error = place(None, &FakeCheckouts::default()).unwrap_err();

    let reason = format!("{error:#}");
    assert!(reason.contains("no clone"), "{reason}");
    assert!(
        reason.contains("https://github.com/acme/shop/pull/7"),
        "{reason}"
    );
    assert!(reason.contains("open the PR from a clone"), "{reason}");
    assert!(!reason.contains("not a git repository"), "{reason}");
}

#[test]
fn git_tells_the_head_and_whether_anything_is_uncommitted() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    repo.write("a.txt", "one\n");
    let head = repo.commit("one");

    assert_eq!(GitCheckouts.head_of(repo.path()).unwrap().as_str(), head);
    assert!(!GitCheckouts.has_uncommitted_changes(repo.path()).unwrap());
    repo.write("untracked.txt", "new\n");
    assert!(GitCheckouts.has_uncommitted_changes(repo.path()).unwrap());
}

#[test]
fn git_says_a_folder_that_is_no_checkout_is_none() {
    let folder = tempfile::TempDir::new().unwrap();

    assert!(GitCheckouts.head_of(folder.path()).is_err());
    assert!(GitCheckouts.has_uncommitted_changes(folder.path()).is_err());
}
