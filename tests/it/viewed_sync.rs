use crate::support::FixtureRepo;
use crate::support::app_path::AppPath;
use crate::support::fake_ports::{FakeGitHub, GitHubCall, GitHubFault, SentLog};
use guided_review::local_git::LocalGit;
use guided_review::pull_request::{PullRequestUrl, RepoName};
use guided_review::tour::Tour;
use guided_review::viewer::{Review, ReviewStateFile, SyncState, ViewedState};
use serde_json::json;

const PR: &str = "https://github.com/acme/shop/pull/7";

const ORDER_BEFORE: &str = "<?php\n\nclass Order\n{\n    public function total(): int\n    {\n        return 1;\n    }\n\n    public function tax(): int\n    {\n        return 1;\n    }\n}\n";
const ORDER_AFTER: &str = "<?php\n\nclass Order\n{\n    public function total(): int\n    {\n        return 2;\n    }\n\n    public function tax(): int\n    {\n        return 2;\n    }\n}\n";

struct Shop {
    repo: FixtureRepo,
    base: String,
    head: String,
    sent: SentLog,
}

impl Shop {
    fn with_pull() -> Self {
        let repo = FixtureRepo::cloned_from("acme/shop");
        repo.write("app/Order.php", ORDER_BEFORE);
        for file in ["a.txt", "b.txt", "c.txt"] {
            repo.write(file, "old\n");
        }
        let base = repo.commit("base");
        repo.write("app/Order.php", ORDER_AFTER);
        for file in ["a.txt", "b.txt", "c.txt"] {
            repo.write(file, "new\n");
        }
        let head = repo.commit("head");
        Self {
            repo,
            base,
            head,
            sent: SentLog::default(),
        }
    }

    fn github(&self) -> FakeGitHub {
        FakeGitHub::with_pull("main-tip", &self.head, &self.sent)
    }

    fn pr(&self) -> PullRequestUrl {
        PR.parse().unwrap()
    }

    fn open(&self) -> Review {
        let tour: Tour = serde_json::from_value(json!({
            "pr": PR,
            "base": self.base,
            "head": self.head,
            "title": "Everything changes",
            "steps": [
                {"id": "a-only", "title": "A", "pieces": [{"file": "a.txt"}]},
                {"id": "a-and-b", "title": "A and B", "pieces": [{"file": "a.txt"}, {"file": "b.txt"}]},
                {"id": "c-only", "title": "C", "pieces": [{"file": "c.txt"}]},
                {"id": "total", "title": "Total", "pieces": [{"file": "app/Order.php", "symbol": "Order::total"}]},
                {"id": "talk", "title": "Talk, no code"},
            ],
        }))
        .unwrap();
        let source = LocalGit::find(&RepoName::new("acme", "shop"), self.repo.path())
            .unwrap()
            .unwrap();
        let state_file = ReviewStateFile::for_pr(self.repo.home(), &self.pr());
        Review::open(tour, &source, state_file).unwrap()
    }
}

fn ticked(review: &Review) -> Vec<String> {
    review
        .sidebar()
        .iter()
        .filter(|entry| entry.is_ticked)
        .map(|entry| entry.id.to_string())
        .collect()
}

fn all_unviewed(shop: &Shop) -> FakeGitHub {
    ["a.txt", "b.txt", "c.txt", "app/Order.php"]
        .iter()
        .fold(shop.github(), |github, path| {
            github.viewing(path, ViewedState::Unviewed)
        })
}

#[test]
fn a_file_is_marked_viewed_once_every_step_holding_it_is_ticked() {
    let shop = Shop::with_pull();
    let github = all_unviewed(&shop);
    let mut review = shop.open();
    review.sync(&github).unwrap();

    review.tick(&"a-only".into()).unwrap();
    review.sync(&github).unwrap();
    assert_eq!(github.viewed_state("a.txt"), ViewedState::Unviewed);

    review.tick(&"a-and-b".into()).unwrap();
    review.sync(&github).unwrap();
    assert_eq!(github.viewed_state("a.txt"), ViewedState::Viewed);
    assert_eq!(github.viewed_state("b.txt"), ViewedState::Viewed);
    assert!(!review.not_synced());
}

#[test]
fn unticking_a_step_unmarks_its_files() {
    let shop = Shop::with_pull();
    let github = all_unviewed(&shop);
    let mut review = shop.open();
    review.tick(&"c-only".into()).unwrap();
    review.sync(&github).unwrap();
    assert_eq!(github.viewed_state("c.txt"), ViewedState::Viewed);

    review.untick(&"c-only".into()).unwrap();
    review.sync(&github).unwrap();

    assert_eq!(github.viewed_state("c.txt"), ViewedState::Unviewed);
}

#[test]
fn a_file_with_changes_no_ticked_step_holds_is_never_marked_viewed() {
    let shop = Shop::with_pull();
    let github = all_unviewed(&shop);
    let mut review = shop.open();

    review.tick(&"total".into()).unwrap();
    review.sync(&github).unwrap();

    assert_eq!(github.viewed_state("app/Order.php"), ViewedState::Unviewed);
}

#[test]
fn ticking_a_step_does_not_unmark_a_file_viewed_on_github() {
    let shop = Shop::with_pull();
    let github = all_unviewed(&shop);
    github.set_viewed("a.txt", ViewedState::Viewed);
    let mut review = shop.open();
    review.sync(&github).unwrap();
    review.tick(&"a-and-b".into()).unwrap();
    review.sync(&github).unwrap();

    assert_eq!(github.viewed_state("a.txt"), ViewedState::Viewed);
}

#[test]
fn a_file_viewed_on_github_ticks_every_step_whose_files_are_all_viewed() {
    let shop = Shop::with_pull();
    let github = all_unviewed(&shop);
    let mut review = shop.open();
    review.sync(&github).unwrap();

    github.set_viewed("a.txt", ViewedState::Viewed);
    review.sync(&github).unwrap();
    assert_eq!(ticked(&review), ["a-only"]);

    github.set_viewed("b.txt", ViewedState::Viewed);
    review.sync(&github).unwrap();
    assert_eq!(ticked(&review), ["a-only", "a-and-b"]);
}

#[test]
fn a_file_not_viewed_any_more_unticks_every_step_holding_it() {
    let shop = Shop::with_pull();
    let github = all_unviewed(&shop);
    github.set_viewed("a.txt", ViewedState::Viewed);
    github.set_viewed("b.txt", ViewedState::Viewed);
    github.set_viewed("c.txt", ViewedState::Viewed);
    let mut review = shop.open();
    review.sync(&github).unwrap();
    assert_eq!(ticked(&review), ["a-only", "a-and-b", "c-only"]);

    github.set_viewed("a.txt", ViewedState::Dismissed);
    review.sync(&github).unwrap();

    assert_eq!(ticked(&review), ["c-only"]);
}

#[test]
fn only_changes_on_github_react_so_a_step_untcked_here_stays_unticked() {
    let shop = Shop::with_pull();
    let github = all_unviewed(&shop);
    github.set_viewed("c.txt", ViewedState::Viewed);
    let mut review = shop.open();
    review.sync(&github).unwrap();
    review.untick(&"c-only".into()).unwrap();
    review.sync(&github).unwrap();
    review.sync(&github).unwrap();

    assert_eq!(ticked(&review), Vec::<String>::new());
    assert_eq!(github.viewed_state("c.txt"), ViewedState::Unviewed);
}

#[test]
fn a_step_without_files_never_ticks_from_github() {
    let shop = Shop::with_pull();
    let github = all_unviewed(&shop);
    let mut review = shop.open();
    review.sync(&github).unwrap();

    for path in ["a.txt", "b.txt", "c.txt", "app/Order.php"] {
        github.set_viewed(path, ViewedState::Viewed);
    }
    review.sync(&github).unwrap();

    assert!(!ticked(&review).contains(&"talk".to_owned()));
}

#[test]
fn a_failed_push_stays_queued_and_is_retried_on_the_next_sync() {
    let shop = Shop::with_pull();
    let github = all_unviewed(&shop);
    github.inject(GitHubFault::ViewedPushesFail);
    let mut review = shop.open();
    review.tick(&"c-only".into()).unwrap();

    assert!(review.sync(&github).is_err());
    assert!(review.not_synced());
    drop(review);

    github.heal(&GitHubFault::ViewedPushesFail);
    let mut review = shop.open();
    assert!(review.not_synced());
    review.sync(&github).unwrap();
    assert!(!review.not_synced());
    assert_eq!(github.viewed_state("c.txt"), ViewedState::Viewed);
}

#[test]
fn a_queued_local_edit_beats_what_github_says() {
    let shop = Shop::with_pull();
    let github = all_unviewed(&shop);
    let mut review = shop.open();
    review.sync(&github).unwrap();
    github.inject(GitHubFault::ViewedPushesFail);
    review.tick(&"c-only".into()).unwrap();
    github.set_viewed("c.txt", ViewedState::Unviewed);
    github.set_viewed("a.txt", ViewedState::Viewed);

    assert!(review.sync(&github).is_err());

    assert_eq!(ticked(&review), ["a-only", "c-only"]);
    github.heal(&GitHubFault::ViewedPushesFail);
    review.sync(&github).unwrap();
    assert_eq!(github.viewed_state("c.txt"), ViewedState::Viewed);
}

#[test]
fn without_access_sync_switches_off_with_a_banner_and_ticks_still_work() {
    let shop = Shop::with_pull();
    let github = all_unviewed(&shop);
    github.inject(GitHubFault::TokenRefused("GitHub refused the token".into()));
    let mut review = shop.open();
    assert_eq!(review.sync_banner(), None);

    review.sync(&github).unwrap();
    assert_eq!(review.sync_banner(), Some("GitHub refused the token"));

    review.tick(&"c-only".into()).unwrap();
    let calls_before = shop.sent.github_calls().len();
    review.sync(&github).unwrap();
    assert_eq!(ticked(&review), ["c-only"]);
    assert_eq!(shop.sent.github_calls().len(), calls_before);
    assert!(!review.not_synced());
}

#[test]
fn nothing_syncs_while_the_tour_is_not_at_the_pull_request_head() {
    let shop = Shop::with_pull();
    let github = FakeGitHub::with_pull("main-tip", "newer-head", &shop.sent)
        .viewing("c.txt", ViewedState::Unviewed)
        .viewing("a.txt", ViewedState::Viewed);
    let mut review = shop.open();
    review.tick(&"c-only".into()).unwrap();

    review.sync(&github).unwrap();

    assert_eq!(ticked(&review), ["c-only"]);
    assert_eq!(github.viewed_state("c.txt"), ViewedState::Unviewed);
    assert!(review.not_synced());
    assert!(
        shop.sent
            .github_calls()
            .iter()
            .all(|call| matches!(call, GitHubCall::TipsOf(_)))
    );
}

#[test]
fn a_sync_asked_for_while_one_runs_runs_once_more_after_it() {
    let mut sync = SyncState::default();

    assert!(sync.start_or_queue());
    assert!(!sync.start_or_queue());
    assert!(!sync.start_or_queue());

    assert!(sync.finish_and_wants_again());
    assert!(sync.start_or_queue());
    assert!(!sync.finish_and_wants_again());
}

#[test]
fn a_sync_that_finishes_with_nothing_asked_leaves_the_next_free_to_start() {
    let mut sync = SyncState::default();
    assert!(sync.start_or_queue());
    assert!(!sync.finish_and_wants_again());
    assert!(sync.start_or_queue());
}
