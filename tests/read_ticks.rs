#[allow(dead_code)]
mod support;

use std::path::PathBuf;

use guided_review::local_git::LocalGit;
use guided_review::pull_request::{PullRequestUrl, RepoName};
use guided_review::tour::Tour;
use guided_review::viewer::{ReadProgress, Review, ReviewStateFile};
use serde_json::{Value, json};
use support::FixtureRepo;

const PR: &str = "https://github.com/acme/shop/pull/7";

struct Shop {
    repo: FixtureRepo,
    base: String,
    head: String,
}

impl Shop {
    fn with_pull() -> Self {
        let repo = FixtureRepo::cloned_from("acme/shop");
        repo.write("README.md", "hello\n");
        let base = repo.commit("base");
        repo.write("README.md", "hello\nworld\n");
        let head = repo.commit("head");
        Self { repo, base, head }
    }

    fn state_path(&self) -> PathBuf {
        self.repo
            .home()
            .join(".guided-review/state/acme/shop/7.json")
    }

    fn state_file(&self) -> ReviewStateFile {
        ReviewStateFile::for_pr(self.repo.home(), &PR.parse::<PullRequestUrl>().unwrap())
    }

    fn open(&self) -> Review {
        self.try_open().unwrap()
    }

    fn try_open(&self) -> anyhow::Result<Review> {
        let tour: Tour = serde_json::from_value(json!({
            "pr": PR,
            "base": self.base,
            "head": self.head,
            "title": "Hello world",
            "steps": [
                {"id": "readme", "title": "Readme", "children": [
                    {"id": "greeting", "title": "Greeting"},
                ]},
                {"id": "emails", "title": "Emails"},
            ],
        }))
        .unwrap();
        let source = LocalGit::find(&RepoName::new("acme", "shop"), self.repo.path())
            .unwrap()
            .unwrap();
        Review::open(tour, &source, self.state_file())
    }

    fn state_on_disk(&self) -> Value {
        serde_json::from_str(&std::fs::read_to_string(self.state_path()).unwrap()).unwrap()
    }
}

fn ticked_in_sidebar(review: &Review) -> Vec<String> {
    review
        .sidebar()
        .iter()
        .filter(|entry| entry.is_ticked)
        .map(|entry| entry.id.to_string())
        .collect()
}

#[test]
fn a_ticked_step_stays_ticked_after_the_app_is_reopened() {
    let shop = Shop::with_pull();
    let mut review = shop.open();

    review.tick(&"greeting".into()).unwrap();
    drop(review);

    assert_eq!(ticked_in_sidebar(&shop.open()), ["greeting"]);
}

#[test]
fn the_state_file_keeps_each_tick_with_the_head_it_was_made_at() {
    let shop = Shop::with_pull();
    let mut review = shop.open();

    review.tick(&"emails".into()).unwrap();

    assert_eq!(
        shop.state_on_disk()["read_ticks"],
        json!({"emails": {"head": shop.head}})
    );
    let leftovers: Vec<_> = std::fs::read_dir(shop.state_path().parent().unwrap())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(leftovers, ["7.json"]);
}

#[test]
fn an_unticked_step_stays_unticked_after_the_app_is_reopened() {
    let shop = Shop::with_pull();
    let mut review = shop.open();
    review.tick(&"readme".into()).unwrap();
    review.tick(&"emails".into()).unwrap();

    review.untick(&"readme".into()).unwrap();
    drop(review);

    assert_eq!(ticked_in_sidebar(&shop.open()), ["emails"]);
}

#[test]
fn progress_counts_ticked_steps_out_of_all_steps() {
    let shop = Shop::with_pull();
    let mut review = shop.open();
    assert_eq!(review.progress(), ReadProgress { read: 0, total: 3 });

    review.tick(&"readme".into()).unwrap();
    review.tick(&"emails".into()).unwrap();

    assert_eq!(review.progress(), ReadProgress { read: 2, total: 3 });
}

#[test]
fn a_ticked_step_folds_and_opens_again_on_request() {
    let shop = Shop::with_pull();
    let mut review = shop.open();
    review.go_to(&"readme".into());
    assert!(!review.current_step().unwrap().is_folded);

    review.tick(&"readme".into()).unwrap();
    assert!(review.current_step().unwrap().is_folded);

    review.toggle_fold(&"readme".into());
    assert!(!review.current_step().unwrap().is_folded);
    assert!(review.is_ticked(&"readme".into()));
}

#[test]
fn ticks_of_steps_the_tour_no_longer_has_are_kept_but_not_counted() {
    let shop = Shop::with_pull();
    std::fs::create_dir_all(shop.state_path().parent().unwrap()).unwrap();
    std::fs::write(
        shop.state_path(),
        json!({"read_ticks": {"dropped": {"head": "old"}}}).to_string(),
    )
    .unwrap();
    let mut review = shop.open();

    review.tick(&"emails".into()).unwrap();

    assert_eq!(review.progress(), ReadProgress { read: 1, total: 3 });
    assert_eq!(
        shop.state_on_disk()["read_ticks"],
        json!({"dropped": {"head": "old"}, "emails": {"head": shop.head}})
    );
}

#[test]
fn sections_a_newer_app_wrote_survive_a_tick() {
    let shop = Shop::with_pull();
    std::fs::create_dir_all(shop.state_path().parent().unwrap()).unwrap();
    std::fs::write(
        shop.state_path(),
        json!({"read_ticks": {}, "asks": [{"question": "why?"}]}).to_string(),
    )
    .unwrap();
    let mut review = shop.open();

    review.tick(&"emails".into()).unwrap();

    assert_eq!(shop.state_on_disk()["asks"], json!([{"question": "why?"}]));
}

#[test]
fn a_broken_state_file_stops_the_app_from_opening_instead_of_being_overwritten() {
    let shop = Shop::with_pull();
    std::fs::create_dir_all(shop.state_path().parent().unwrap()).unwrap();
    std::fs::write(shop.state_path(), "{ not json").unwrap();

    let error = shop.try_open().err().expect("opening should fail");

    assert!(
        format!("{error:#}").contains("is not a valid review state file"),
        "error: {error:#}"
    );
    assert_eq!(
        std::fs::read_to_string(shop.state_path()).unwrap(),
        "{ not json"
    );
}
