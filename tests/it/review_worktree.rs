use guided_review::pull_request::Sha;
use guided_review::review_subject::ReviewSubject;
use guided_review::review_worktree::GitReviewWorktrees;

use crate::support::FixtureRepo;

fn subject(number: u32) -> ReviewSubject {
    format!("https://github.com/acme/shop/pull/{number}")
        .parse()
        .unwrap()
}

/// A clone with worktrees the way older builds made them, one per subject under the app's folder.
struct Shop {
    repo: FixtureRepo,
    worktrees: GitReviewWorktrees,
    reviewed: Sha,
}

impl Shop {
    fn with_a_head_the_clone_has_moved_past() -> Self {
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

    fn left_by_an_older_build(&self, number: u32) -> std::path::PathBuf {
        let path = self
            .repo
            .folders()
            .review_worktrees()
            .join(subject(number).files_folder())
            .join(subject(number).file_stem());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        self.repo.git(&[
            "worktree",
            "add",
            "--detach",
            path.to_str().unwrap(),
            self.reviewed.as_str(),
        ]);
        path
    }
}

#[test]
fn the_sweep_removes_leftovers_but_keeps_the_subjects_it_is_told_to() {
    let shop = Shop::with_a_head_the_clone_has_moved_past();
    let leftover = shop.left_by_an_older_build(7);
    let kept = shop.left_by_an_older_build(8);

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
    let shop = Shop::with_a_head_the_clone_has_moved_past();

    assert_eq!(shop.worktrees.sweep(&[]), Vec::<String>::new());
}
