use guided_review::pull_request::Sha;
use guided_review::review_subject::ReviewSubject;
use guided_review::review_worktree::{GitReviewWorktrees, ReviewWorktreePort};

use crate::support::FixtureRepo;
use crate::support::fake_review_worktrees::{FakeReviewWorktrees, WorktreeCall};

fn subject(number: u32) -> ReviewSubject {
    format!("https://github.com/acme/shop/pull/{number}")
        .parse()
        .unwrap()
}

struct Shop {
    repo: FixtureRepo,
    worktrees: GitReviewWorktrees,
    reviewed: Sha,
}

impl Shop {
    fn reviewing_a_head_that_the_clone_has_moved_past() -> Self {
        let repo = FixtureRepo::cloned_from("acme/shop");
        repo.write("a.txt", "at head\n");
        let reviewed = Sha::new(&repo.commit("head"));
        repo.write("a.txt", "moved on\n");
        repo.commit("later");
        let worktrees = GitReviewWorktrees::under(repo.folders().review_worktrees());
        Self {
            repo,
            worktrees,
            reviewed,
        }
    }

    fn add(&self, number: u32) -> std::path::PathBuf {
        self.worktrees
            .add(&subject(number), self.repo.path(), &self.reviewed)
            .unwrap()
            .as_path()
            .to_path_buf()
    }
}

#[test]
fn the_worktree_holds_the_reviewed_head_outside_the_clone() {
    let shop = Shop::reviewing_a_head_that_the_clone_has_moved_past();

    let path = shop.add(7);

    assert_eq!(
        std::fs::read_to_string(path.join("a.txt")).unwrap(),
        "at head\n"
    );
    assert!(!path.starts_with(shop.repo.path()), "{}", path.display());
    assert!(path.starts_with(shop.repo.folders().review_worktrees()));
    assert_eq!(
        std::fs::read_to_string(shop.repo.path().join("a.txt")).unwrap(),
        "moved on\n"
    );
}

#[test]
fn removing_it_deletes_the_folder_and_git_forgets_it() {
    let shop = Shop::reviewing_a_head_that_the_clone_has_moved_past();
    let path = shop.add(7);

    shop.worktrees.remove(&subject(7)).unwrap();

    assert!(!path.exists());
    assert_eq!(
        shop.repo
            .git(&["worktree", "list", "--porcelain"])
            .matches("worktree ")
            .count(),
        1
    );
}

#[test]
fn removing_one_that_is_not_there_is_fine() {
    let shop = Shop::reviewing_a_head_that_the_clone_has_moved_past();

    shop.worktrees.remove(&subject(7)).unwrap();
}

#[test]
fn a_second_run_replaces_the_checkout_an_earlier_one_left() {
    let shop = Shop::reviewing_a_head_that_the_clone_has_moved_past();
    let first = shop.add(7);
    std::fs::write(first.join("a.txt"), "the agent's mess\n").unwrap();

    let second = shop.add(7);

    assert_eq!(first, second);
    assert_eq!(
        std::fs::read_to_string(second.join("a.txt")).unwrap(),
        "at head\n"
    );
}

#[test]
fn the_sweep_removes_leftovers_but_keeps_the_subjects_it_is_told_to() {
    let shop = Shop::reviewing_a_head_that_the_clone_has_moved_past();
    let leftover = shop.add(7);
    let kept = shop.add(8);

    let not_swept = shop.worktrees.sweep(&[subject(8)]);

    assert_eq!(not_swept, Vec::<String>::new());
    assert!(!leftover.exists());
    assert!(kept.exists());
    assert_eq!(
        shop.repo
            .git(&["worktree", "list", "--porcelain"])
            .matches("worktree ")
            .count(),
        2
    );
}

#[test]
fn the_sweep_of_a_folder_that_is_not_there_has_nothing_to_do() {
    let shop = Shop::reviewing_a_head_that_the_clone_has_moved_past();

    assert_eq!(shop.worktrees.sweep(&[]), Vec::<String>::new());
}

#[test]
fn a_pr_with_no_clone_fails_with_what_to_do_not_with_gits_message() {
    let shop = Shop::reviewing_a_head_that_the_clone_has_moved_past();
    let no_clone = tempfile::TempDir::new().unwrap();

    let error = shop
        .worktrees
        .add(&subject(7), no_clone.path(), &shop.reviewed)
        .unwrap_err();

    let reason = format!("{error:#}");
    assert!(reason.contains("no clone"), "{reason}");
    assert!(reason.contains("https://github.com/acme/shop/pull/7"), "{reason}");
    assert!(reason.contains("open the PR from a clone"), "{reason}");
    assert!(!reason.contains("not a git repository"), "{reason}");
}

#[test]
fn the_fake_keeps_the_record_a_test_of_the_agent_reads() {
    let fake = FakeReviewWorktrees::default();
    let repo = std::path::Path::new("/clones/shop");

    fake.add(&subject(7), repo, &Sha::new("abc")).unwrap();
    fake.add(&subject(8), repo, &Sha::new("def")).unwrap();
    fake.remove(&subject(7)).unwrap();

    assert_eq!(fake.standing(), vec![subject(8).to_string()]);
    assert_eq!(fake.calls().len(), 3);
    assert!(matches!(&fake.calls()[0], WorktreeCall::Add { head, .. } if head == "abc"));
    assert!(
        FakeReviewWorktrees::refusing()
            .add(&subject(7), repo, &Sha::new("abc"))
            .is_err()
    );
}
