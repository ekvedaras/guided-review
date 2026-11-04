#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "a panic in a test helper is the test failing"
)]
#![allow(
    clippy::needless_pass_by_value,
    reason = "test helpers take owned arguments so a case reads as a plain call"
)]

#[allow(
    dead_code,
    reason = "each test binary uses only part of the shared test support"
)]
mod support;

use guided_review::local_git::LocalGit;
use guided_review::pull_request::RepoName;
use guided_review::tour::Tour;
use guided_review::viewer::{JumpKind, Review, ReviewStateFile, Screen};
use serde_json::{Value, json};
use support::FixtureRepo;

fn open(steps: Vec<Value>) -> Review {
    let repo = FixtureRepo::cloned_from("acme/shop");
    repo.write(
        "app/Order.php",
        "<?php\n\nclass Order\n{\n    public function total(): int\n    {\n        return 1;\n    }\n}\n",
    );
    repo.write("README.md", "hello\n");
    let base = repo.commit("base");
    repo.write(
        "app/Order.php",
        "<?php\n\nclass Order\n{\n    public function total(): int\n    {\n        return 2;\n    }\n}\n",
    );
    repo.write("README.md", "hello\nworld\n");
    let head = repo.commit("head");
    let tour: Tour = serde_json::from_value(json!({
        "pr": "https://github.com/acme/shop/pull/7",
        "base": base,
        "head": head,
        "title": "Orders cost two",
        "summary": [],
        "steps": steps,
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

fn step(id: &str, title: &str, pieces: Value) -> Value {
    json!({"id": id, "title": title, "why": "because", "pieces": pieces})
}

fn shop_tour() -> Vec<Value> {
    vec![
        step(
            "totals",
            "Order totals go up",
            json!([{"file": "app/Order.php", "symbol": "Order::total"}]),
        ),
        step("docs", "Readme says hello", json!([{"file": "README.md"}])),
    ]
}

fn labels(review: &Review, query: &str) -> Vec<String> {
    review
        .jump_targets(query)
        .iter()
        .map(|target| format!("{:?} {}", target.kind, target.label))
        .collect()
}

#[test]
fn an_empty_query_lists_steps_files_and_symbols_in_reading_order() {
    let review = open(shop_tour());

    assert_eq!(
        labels(&review, ""),
        [
            "Step Order totals go up",
            "File app/Order.php",
            "Symbol app/Order.php#Order::total",
            "Step Readme says hello",
            "File README.md",
        ]
    );
}

#[test]
fn a_query_matches_letters_in_order_ignoring_case() {
    let review = open(shop_tour());

    assert_eq!(
        labels(&review, "ordtot"),
        [
            "Step Order totals go up",
            "Symbol app/Order.php#Order::total"
        ]
    );
    assert_eq!(
        labels(&review, "README"),
        ["File README.md", "Step Readme says hello"]
    );
    assert_eq!(labels(&review, "zzz"), [] as [String; 0]);
}

#[test]
fn letters_scattered_inside_words_match_nothing() {
    let review = open(shop_tour());

    assert_eq!(labels(&review, "rdtl"), [] as [String; 0]);
    assert_eq!(
        labels(&review, "otals"),
        ["Step Order totals go up"],
        "a run found whole in the text matches anywhere"
    );
}

#[test]
fn the_closest_match_comes_first() {
    let review = open(shop_tour());

    assert_eq!(labels(&review, "total")[0], "Step Order totals go up");
    assert_eq!(
        labels(&review, "Order::total")[0],
        "Symbol app/Order.php#Order::total"
    );
}

#[test]
fn picking_a_target_opens_the_step_that_holds_it() {
    let mut review = open(shop_tour());

    let file = review
        .jump_targets("README")
        .into_iter()
        .find(|target| target.kind == JumpKind::File)
        .unwrap();
    review.jump_to(&file);

    assert_eq!(review.screen(), &Screen::Step("docs".into()));
}

#[test]
fn a_symbol_target_names_its_file_and_step() {
    let review = open(shop_tour());

    let symbol = review
        .jump_targets("Order::total")
        .into_iter()
        .find(|target| target.kind == JumpKind::Symbol)
        .unwrap();

    assert_eq!(symbol.screen, Screen::Step("totals".into()));
    assert_eq!(symbol.detail, "Order totals go up");
}

#[test]
fn picking_a_target_in_a_folded_step_unfolds_it_for_the_session() {
    let mut review = open(shop_tour());
    let skim = "totals".into();
    review.toggle_fold(&skim);
    assert!(review.is_folded(&skim));

    let symbol = review.jump_targets("Order::total")[0].clone();
    review.jump_to(&symbol);

    assert!(!review.is_folded(&skim));
}

#[test]
fn picking_a_target_in_an_open_step_leaves_it_open() {
    let mut review = open(shop_tour());
    let id = "totals".into();

    let symbol = review.jump_targets("Order::total")[0].clone();
    review.jump_to(&symbol);

    assert!(!review.is_folded(&id));
}

#[test]
fn files_only_the_safety_net_shows_are_jump_targets() {
    let mut review = open(vec![step(
        "totals",
        "Order totals go up",
        json!([{"file": "app/Order.php", "symbol": "Order::total"}]),
    )]);

    let readme = review
        .jump_targets("README")
        .into_iter()
        .find(|target| target.kind == JumpKind::File)
        .unwrap();
    assert_eq!(readme.detail, "Safety net");

    review.jump_to(&readme);
    assert_eq!(review.screen(), &Screen::SafetyNet);
}
