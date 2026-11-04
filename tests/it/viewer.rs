use crate::support::FixtureRepo;
use guided_review::local_git::LocalGit;
use guided_review::pull_request::RepoName;
use guided_review::tour::Tour;
use guided_review::viewer::{NextScreen, ReadingPlace, Review, ReviewStateFile, Screen, Side};
use serde_json::{Value, json};

struct Shop {
    repo: FixtureRepo,
    base: String,
    head: String,
}

impl Shop {
    fn with_pull() -> Self {
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
        Self { repo, base, head }
    }

    fn open(&self, steps: Vec<Value>) -> Review {
        let tour: Tour = serde_json::from_value(json!({
            "pr": "https://github.com/acme/shop/pull/7",
            "base": self.base,
            "head": self.head,
            "title": "Orders cost two",
            "summary": ["Order total goes up", "README says hello world"],
            "steps": steps,
        }))
        .unwrap();
        let source = LocalGit::find(&RepoName::new("acme", "shop"), self.repo.path())
            .unwrap()
            .unwrap();
        let state_file = ReviewStateFile::for_pr(
            self.repo.home(),
            &"https://github.com/acme/shop/pull/7".parse().unwrap(),
        );
        Review::open(tour, &source, state_file).unwrap()
    }
}

fn step(id: &str, pieces: Value, children: Vec<Value>) -> Value {
    json!({"id": id, "title": format!("Title of {id}"), "why": "because", "pieces": pieces, "children": children})
}

#[test]
fn first_screen_shows_only_the_summary() {
    let review = Shop::with_pull().open(vec![step("order", json!([]), vec![])]);

    assert_eq!(review.screen(), &Screen::Summary);
    assert_eq!(review.title(), "Orders cost two");
    assert_eq!(
        review.summary(),
        ["Order total goes up", "README says hello world"]
    );
    assert!(review.current_step().is_none());
}

fn nested_tour() -> Vec<Value> {
    vec![
        step(
            "checkout",
            json!([]),
            vec![
                step(
                    "pricing",
                    json!([]),
                    vec![step("rounding", json!([]), vec![])],
                ),
                step("payment", json!([]), vec![]),
            ],
        ),
        step("emails", json!([]), vec![]),
    ]
}

fn read_forward(review: &mut Review) -> Vec<String> {
    let mut seen = Vec::new();
    while review.go_forward() {
        match review.screen() {
            Screen::Step(id) => seen.push(id.to_string()),
            Screen::SafetyNet => seen.push("safety net".to_owned()),
            Screen::Summary => panic!("going forward never reaches to the summary"),
        }
    }
    seen
}

#[test]
fn going_forward_reads_a_parent_then_its_children_then_the_next_parent() {
    let mut review = Shop::with_pull().open(nested_tour());

    assert_eq!(
        read_forward(&mut review),
        [
            "checkout",
            "pricing",
            "rounding",
            "payment",
            "emails",
            "safety net"
        ]
    );
}

#[test]
fn going_back_walks_back_to_the_summary() {
    let mut review = Shop::with_pull().open(nested_tour());
    review.go_forward();
    review.go_forward();

    assert!(review.go_back());
    assert_eq!(review.screen(), &Screen::Step("checkout".into()));
    assert!(review.go_back());
    assert_eq!(review.screen(), &Screen::Summary);
    assert!(!review.go_back());
}

#[test]
fn the_reading_place_numbers_the_step_and_names_the_screen_after_it() {
    let mut review = Shop::with_pull().open(nested_tour());
    let next_step = |id: &str| {
        Some(NextScreen::Step {
            title: format!("Title of {id}"),
        })
    };

    assert_eq!(
        review.reading_place(),
        ReadingPlace {
            step_number: None,
            step_count: 5,
            has_previous: false,
            next: next_step("checkout"),
        }
    );
    review.go_forward();
    review.go_forward();
    assert_eq!(
        review.reading_place(),
        ReadingPlace {
            step_number: Some(2),
            step_count: 5,
            has_previous: true,
            next: next_step("rounding"),
        }
    );
    review.go_to(&"emails".into());
    assert_eq!(review.reading_place().step_number, Some(5));
    assert_eq!(review.reading_place().next, Some(NextScreen::SafetyNet));
    review.go_forward();
    assert_eq!(review.reading_place().step_number, None);
    assert_eq!(review.reading_place().next, None);
}

fn sidebar_lines(review: &Review) -> Vec<String> {
    review
        .sidebar()
        .iter()
        .map(|entry| {
            format!(
                "{}{}{}",
                "  ".repeat(entry.depth),
                entry.title,
                if entry.is_current { " <" } else { "" }
            )
        })
        .collect()
}

#[test]
fn sidebar_shows_the_step_tree_in_reading_order_and_marks_where_i_am() {
    let mut review = Shop::with_pull().open(nested_tour());
    review.go_forward();
    review.go_forward();

    assert_eq!(
        sidebar_lines(&review),
        [
            "Title of checkout",
            "  Title of pricing <",
            "    Title of rounding",
            "  Title of payment",
            "Title of emails",
        ]
    );
}

#[test]
fn picking_a_step_in_the_sidebar_shows_it_and_going_forward_goes_on_from_there() {
    let mut review = Shop::with_pull().open(nested_tour());

    review.go_to(&"payment".into());

    assert_eq!(review.screen(), &Screen::Step("payment".into()));
    assert_eq!(
        review.current_step().unwrap().step.title,
        "Title of payment"
    );
    review.go_forward();
    assert_eq!(review.screen(), &Screen::Step("emails".into()));
}

fn shown_lines(review: &Review) -> Vec<(String, Vec<String>)> {
    review
        .current_step()
        .unwrap()
        .pieces
        .iter()
        .map(|piece| {
            let lines = piece
                .lines()
                .map(|line| {
                    let side = match line.location.side {
                        Side::Base => "base",
                        Side::Head => "head",
                    };
                    format!(
                        "{}:{side}:{} {}",
                        line.location.path, line.location.line, line.text
                    )
                })
                .collect();
            (piece.id.to_string(), lines)
        })
        .collect()
}

#[test]
fn a_step_shows_its_pieces_with_path_side_and_line_on_every_line() {
    let mut review = Shop::with_pull().open(vec![step(
        "order",
        json!([{"file": "app/Order.php", "symbol": "Order::total"}, {"file": "README.md"}]),
        vec![],
    )]);
    review.go_forward();

    assert_eq!(
        shown_lines(&review),
        [
            (
                "app/Order.php#Order::total".to_owned(),
                vec![
                    "app/Order.php:base:7         return 1;".to_owned(),
                    "app/Order.php:head:7         return 2;".to_owned(),
                ]
            ),
            (
                "README.md#hunk-1".to_owned(),
                vec!["README.md:head:2 world".to_owned()]
            ),
        ]
    );
}

#[test]
fn a_rest_pointer_shows_the_changes_under_its_folder_that_no_other_step_shows() {
    let mut review = Shop::with_pull().open(vec![
        step("readme", json!([{"file": "README.md"}]), vec![]),
        step("rest", json!([{"rest": ""}]), vec![]),
    ]);
    review.go_to(&"rest".into());

    let shown: Vec<String> = shown_lines(&review).into_iter().map(|(id, _)| id).collect();
    assert_eq!(shown, ["app/Order.php#Order::total"]);
}
