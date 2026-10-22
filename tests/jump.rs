#[allow(dead_code)]
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
    assert!(labels(&review, "zzz").is_empty());
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

    assert_eq!(symbol.step, "totals".into());
    assert_eq!(symbol.detail, "Order totals go up");
}

#[test]
fn a_piece_opens_in_the_ide_at_its_first_changed_line_at_head() {
    let mut review = open(shop_tour());
    review.jump_to(&review.jump_targets("Order::total")[0].clone());

    let page = review.current_step().unwrap();
    let at = page.pieces[0].open_at().unwrap();

    assert_eq!((at.path.as_str(), at.line), ("app/Order.php", 7));
}
