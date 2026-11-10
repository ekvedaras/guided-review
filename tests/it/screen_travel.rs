use crate::support::FixtureRepo;
use guided_review::local_git::LocalGit;
use guided_review::pull_request::RepoName;
use guided_review::tour::Tour;
use guided_review::viewer::{Review, ReviewStateFile, ScreenTravel, Travel};
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

#[test]
fn the_first_screen_arrives_going_forward() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    let review = three_step_review(&repo);
    let mut travel = ScreenTravel::default();

    travel.arrive(&review);

    assert_eq!(travel.heading(), Travel::Forward);
}

#[test]
fn going_on_heads_forward_and_going_back_heads_back() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    let mut review = three_step_review(&repo);
    let mut travel = ScreenTravel::default();
    travel.arrive(&review);

    review.go_to(&"three".into());
    travel.arrive(&review);
    assert_eq!(travel.heading(), Travel::Forward);

    review.go_to(&"one".into());
    travel.arrive(&review);
    assert_eq!(travel.heading(), Travel::Back);

    review.go_to_summary();
    travel.arrive(&review);
    assert_eq!(travel.heading(), Travel::Back);
}

#[test]
fn each_new_screen_is_a_new_arrival_and_staying_is_not() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    let mut review = three_step_review(&repo);
    let mut travel = ScreenTravel::default();
    travel.arrive(&review);
    let on_summary = travel.arrival();

    travel.arrive(&review);
    assert_eq!(travel.arrival(), on_summary, "the same screen again");

    review.go_forward();
    travel.arrive(&review);
    let on_one = travel.arrival();
    assert_ne!(on_one, on_summary);

    review.go_back();
    travel.arrive(&review);
    assert_ne!(
        travel.arrival(),
        on_summary,
        "coming back is a new arrival, so its entrance plays again"
    );
}
