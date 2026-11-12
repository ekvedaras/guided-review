use crate::support::FixtureRepo;
use guided_review::local_git::LocalGit;
use guided_review::pull_request::RepoName;
use guided_review::tour::Tour;
use guided_review::viewer::{Effect, Intent, Review, ReviewStateFile, Screen, WindowMoment};
use serde_json::json;

fn three_step_review(repo: &FixtureRepo) -> Review {
    repo.write("README.md", "hello\n");
    let base = repo.commit("base");
    repo.write("README.md", "hello\nworld\n");
    let head = repo.commit("head");
    let tour: Tour = serde_json::from_value(json!({
        "pr": "https://github.com/acme/shop/pull/7",
        "base": base,
        "head": head,
        "title": "Hello world",
        "steps": [
            {"id": "one", "title": "One"},
            {"id": "two", "title": "Two"},
            {"id": "three", "title": "Three"},
        ],
    }))
    .unwrap();
    let source = LocalGit::find(&RepoName::new("acme", "shop"), repo.path())
        .unwrap()
        .unwrap();
    let state_file = ReviewStateFile::for_pr(
        repo.home(),
        &"https://github.com/acme/shop/pull/7".parse().unwrap(),
    );
    Review::open(tour, &source, state_file).unwrap()
}

fn act(review: &mut Review, intent: Intent) {
    let Effect::Change(change) = review.decide(intent, WindowMoment::default());
    review.apply(change).unwrap();
}

fn on_step(review: &mut Review, id: &str) {
    review.go_to(&id.into());
}

#[test]
fn the_forward_key_does_not_tick_a_step_whose_end_was_not_seen() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    let mut review = three_step_review(&repo);
    on_step(&mut review, "one");

    act(&mut review, Intent::NextScreen);

    assert!(!review.is_ticked(&"one".into()));
    assert_eq!(review.screen(), &Screen::Step("two".into()));
}

#[test]
fn the_forward_key_ticks_a_step_whose_end_was_seen() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    let mut review = three_step_review(&repo);
    on_step(&mut review, "one");

    review.saw_end_of_screen();
    act(&mut review, Intent::NextScreen);

    assert!(review.is_ticked(&"one".into()));
    assert_eq!(review.screen(), &Screen::Step("two".into()));
}

#[test]
fn an_end_seen_on_another_screen_does_not_let_the_forward_key_tick() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    let mut review = three_step_review(&repo);
    on_step(&mut review, "one");
    review.saw_end_of_screen();
    on_step(&mut review, "two");

    act(&mut review, Intent::NextScreen);

    assert!(!review.is_ticked(&"two".into()));
}

#[test]
fn the_footers_mark_as_read_ticks_without_the_end_seen() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    let mut review = three_step_review(&repo);
    on_step(&mut review, "one");

    act(&mut review, Intent::MarkReadAndGoOn);

    assert!(
        review.is_ticked(&"one".into()),
        "unlike the forward key, the footer's button does not ask whether the end was seen"
    );
    assert_eq!(review.screen(), &Screen::Step("two".into()));
}

#[test]
fn going_on_from_a_step_read_already_keeps_its_tick() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    let mut review = three_step_review(&repo);
    on_step(&mut review, "one");
    review.tick(&"one".into()).unwrap();

    review.saw_end_of_screen();
    act(&mut review, Intent::NextScreen);
    on_step(&mut review, "one");
    act(&mut review, Intent::MarkReadAndGoOn);

    assert!(review.is_ticked(&"one".into()));
}
