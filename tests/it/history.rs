//! Back and Forward: every change of screen is remembered, like a browser's history.

use crate::support::FixtureRepo;
use guided_review::local_git::LocalGit;
use guided_review::tour::GuidedTour;
use guided_review::viewer::{
    Effect, Intent, PageRow, Reopening, Review, ReviewStateFile, Screen, WindowMoment,
};
use serde_json::json;

fn three_step_review(repo: &FixtureRepo) -> Review {
    repo.write("README.md", "hello\n");
    let base = repo.commit("base");
    repo.write("README.md", "hello\nworld\n");
    let head = repo.commit("head");
    let tour: GuidedTour = serde_json::from_value(json!({
        "pr": "https://github.com/acme/shop/pull/7",
        "base": base,
        "head": head,
        "title": "Hello world",
        "steps": [
            {"id": "one", "title": "One", "pieces": [{"file": "README.md"}]},
            {"id": "two", "title": "Two", "pieces": [{"file": "README.md"}]},
            {"id": "three", "title": "Three"},
        ],
    }))
    .unwrap();
    let source = LocalGit::find(&crate::support::shop_on_github(), repo.path())
        .unwrap()
        .unwrap();
    let state_file = ReviewStateFile::for_subject(
        &repo.folders(),
        &"https://github.com/acme/shop/pull/7".parse().unwrap(),
    );
    let mut review = Review::open(
        tour,
        &source,
        state_file,
        crate::support::fake_ports::every_capability(),
    )
    .unwrap();
    let files_at_head = review.files_at_head_request().read(&source);
    review.set_files_at_head(files_at_head);
    review
}

fn step(id: &str) -> Screen {
    Screen::Step(id.into())
}

/// The row of the first line of code the open screen shows, below its header.
fn first_line_row(review: &Review) -> usize {
    let row = review
        .page()
        .rows()
        .iter()
        .position(|row| matches!(row, PageRow::Line(_)))
        .unwrap();
    assert!(row > 0, "a header comes first");
    row
}

#[test]
fn back_returns_to_the_screen_a_jump_left() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    let mut review = three_step_review(&repo);
    review.go_to(&"three".into());

    assert!(review.go_back_in_history());

    assert_eq!(review.screen(), &Screen::Summary);
}

#[test]
fn back_walks_the_screens_in_the_order_they_were_visited() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    let mut review = three_step_review(&repo);
    review.go_to(&"three".into());
    review.go_to(&"one".into());

    review.go_back_in_history();
    assert_eq!(review.screen(), &step("three"));
    review.go_back_in_history();
    assert_eq!(review.screen(), &Screen::Summary);
}

#[test]
fn back_with_nothing_behind_stays_put() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    let mut review = three_step_review(&repo);

    assert!(!review.go_back_in_history());

    assert_eq!(review.screen(), &Screen::Summary);
}

#[test]
fn the_next_and_previous_screen_keys_are_remembered_too() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    let mut review = three_step_review(&repo);
    review.go_forward();
    review.go_forward();

    review.go_back_in_history();
    assert_eq!(review.screen(), &step("one"));
    review.go_back_in_history();
    assert_eq!(review.screen(), &Screen::Summary);
}

#[test]
fn next_unread_and_going_on_are_remembered() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    let mut review = three_step_review(&repo);
    review.go_on();
    review.go_to_next_unread();

    review.go_back_in_history();
    assert_eq!(review.screen(), &step("one"));
}

#[test]
fn forward_undoes_a_back() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    let mut review = three_step_review(&repo);
    review.go_to(&"three".into());
    review.go_back_in_history();

    assert!(review.go_forward_in_history());

    assert_eq!(review.screen(), &step("three"));
    assert!(!review.go_forward_in_history());
}

#[test]
fn a_new_move_after_going_back_drops_what_forward_held() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    let mut review = three_step_review(&repo);
    review.go_to(&"three".into());
    review.go_back_in_history();

    review.go_to(&"two".into());

    assert!(!review.go_forward_in_history());
    assert_eq!(review.screen(), &step("two"));
}

#[test]
fn going_back_and_forward_does_not_add_visits() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    let mut review = three_step_review(&repo);
    review.go_to(&"three".into());
    review.go_back_in_history();
    review.go_forward_in_history();
    review.go_back_in_history();

    assert_eq!(review.screen(), &Screen::Summary);
    assert!(!review.go_back_in_history());
}

#[test]
fn going_to_the_screen_already_open_is_not_a_visit() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    let mut review = three_step_review(&repo);
    review.go_to(&"two".into());
    review.go_to(&"two".into());

    review.go_back_in_history();

    assert_eq!(review.screen(), &Screen::Summary);
}

#[test]
fn back_returns_to_where_the_reviewer_was_scrolled() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    let mut review = three_step_review(&repo);
    review.go_to(&"one".into());
    let row = first_line_row(&review);
    review.note_scroll_top(row);
    review.go_to(&"three".into());

    review.go_back_in_history();

    assert_eq!(review.take_row_to_scroll_to(), Some(row));
    assert_eq!(review.take_row_to_scroll_to(), None, "once");
}

#[test]
fn back_returns_to_the_line_a_jump_on_the_same_screen_left() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    let mut review = three_step_review(&repo);
    review.go_to(&"one".into());
    let row = first_line_row(&review);
    review.note_scroll_top(row);
    let target = review
        .jump_targets("README")
        .into_iter()
        .find(|target| target.screen == step("one"))
        .unwrap();

    review.jump_to(&target);
    review.note_scroll_top(row + 1);
    assert!(review.go_back_in_history());

    assert_eq!(review.screen(), &step("one"));
    assert_eq!(review.take_row_to_scroll_to(), Some(row));
}

#[test]
fn forward_returns_to_where_the_reviewer_was_when_they_went_back() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    let mut review = three_step_review(&repo);
    review.go_to(&"one".into());
    review.go_to(&"two".into());
    let row = first_line_row(&review);
    review.note_scroll_top(row);
    review.go_back_in_history();
    review.take_row_to_scroll_to();

    review.go_forward_in_history();

    assert_eq!(review.take_row_to_scroll_to(), Some(row));
}

#[test]
fn a_move_that_is_not_back_restores_nothing() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    let mut review = three_step_review(&repo);
    review.go_to(&"one".into());
    review.note_scroll_top(first_line_row(&review));
    review.go_to(&"three".into());

    assert_eq!(review.take_row_to_scroll_to(), None);
}

#[test]
fn a_scroll_noted_on_one_screen_is_not_carried_to_the_next_visit() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    let mut review = three_step_review(&repo);
    review.go_to(&"one".into());
    review.note_scroll_top(first_line_row(&review));
    review.go_to(&"two".into());
    review.go_to(&"three".into());

    review.go_back_in_history();

    assert_eq!(review.screen(), &step("two"));
    assert_eq!(review.take_row_to_scroll_to(), Some(0));
}

#[test]
fn the_history_goes_on_in_the_review_that_replaces_this_one_after_a_re_pin() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    let mut review = three_step_review(&repo);
    review.go_to(&"three".into());
    let mut replacement = three_step_review(&repo);

    replacement.carry_on_from(review);
    replacement.resume_on(&step("three"));

    assert_eq!(replacement.screen(), &step("three"));
    assert!(replacement.go_back_in_history());
    assert_eq!(replacement.screen(), &Screen::Summary);
}

fn act(review: &mut Review, intent: Intent) {
    match review.decide(intent, WindowMoment::default()) {
        Effect::Change(change) => review.apply(change).unwrap(),
        other => panic!("{intent:?} changed nothing: {other:?}"),
    }
}

#[test]
fn the_back_and_forward_intents_go_through_the_history() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    let mut review = three_step_review(&repo);
    review.go_to(&"three".into());

    act(&mut review, Intent::HistoryBack);
    assert_eq!(review.screen(), &Screen::Summary);

    act(&mut review, Intent::HistoryForward);
    assert_eq!(review.screen(), &step("three"));
}

#[test]
fn the_back_intent_with_nothing_behind_does_nothing() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    let review = three_step_review(&repo);

    assert_eq!(
        review.decide(Intent::HistoryBack, WindowMoment::default()),
        Effect::Nothing
    );
    assert_eq!(
        review.decide(Intent::HistoryForward, WindowMoment::default()),
        Effect::Nothing
    );
}

#[test]
fn back_is_refused_while_the_review_is_reopening() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    let mut review = three_step_review(&repo);
    review.go_to(&"three".into());
    let reopening = WindowMoment {
        reopening: Reopening::Underway,
        ..WindowMoment::default()
    };

    assert!(matches!(
        review.decide(Intent::HistoryBack, reopening),
        Effect::Refused(_)
    ));
}
