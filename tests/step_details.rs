#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "a panic in a test helper is the test failing"
)]

#[allow(dead_code)]
mod support;

use guided_review::local_git::LocalGit;
use guided_review::pull_request::{PullRequestUrl, RepoName};
use guided_review::tour::Tour;
use guided_review::viewer::{PieceDiff, Review, ReviewStateFile, Screen, Side};
use serde_json::{Value, json};
use support::app_path::AppPath;
use support::fake_ports::{FakeGitHub, GitHubCall, SentLog};
use support::{FixtureRepo, on_github};

struct Shop {
    repo: FixtureRepo,
    base: String,
    head: String,
}

fn config(lines: &[(usize, &str)]) -> String {
    (1..=30)
        .map(|number| {
            lines
                .iter()
                .find(|(changed, _)| *changed == number)
                .map_or(format!("key{number}: {number}\n"), |(_, text)| {
                    format!("{text}\n")
                })
        })
        .collect()
}

impl Shop {
    fn with_pull() -> Self {
        let repo = FixtureRepo::cloned_from("acme/shop");
        repo.write(
            "app/Order.php",
            "<?php\n\nclass Order\n{\n    public function total(): int\n    {\n        return 1;\n    }\n}\n",
        );
        repo.write("config.yml", &config(&[]));
        repo.write("README.md", "hello\n");
        repo.write("notes/old.txt", "one\ntwo\nthree\nfour\nfive\nsix\n");
        let base = repo.commit("base");
        repo.write(
            "app/Order.php",
            "<?php\n\nclass Order\n{\n    public function total(): int\n    {\n        return 2;\n    }\n\n    public function discount(): int\n    {\n        return 0;\n    }\n}\n",
        );
        repo.write(
            "config.yml",
            &config(&[(3, "key3: three"), (20, "key20: twenty")]),
        );
        repo.write("README.md", "hello\nworld\n");
        std::fs::write(repo.path().join("data.bin"), [0u8, 159, 146, 150, 0, 1]).unwrap();
        repo.git(&["mv", "notes/old.txt", "notes/new.txt"]);
        repo.write("notes/new.txt", "one\ntwo\nthree\nfour\nfive\nSIX\n");
        let head = repo.commit("head");
        Self { repo, base, head }
    }

    fn tour(&self, steps: Vec<Value>) -> Tour {
        serde_json::from_value(json!({
            "pr": "https://github.com/acme/shop/pull/7",
            "base": self.base,
            "head": self.head,
            "title": "Orders cost two",
            "summary": ["Order total goes up"],
            "steps": steps,
        }))
        .unwrap()
    }

    fn open(&self, steps: Vec<Value>) -> Review {
        let source = LocalGit::find(&RepoName::new("acme", "shop"), self.repo.path())
            .unwrap()
            .unwrap();
        let state_file = ReviewStateFile::for_pr(
            self.repo.home(),
            &"https://github.com/acme/shop/pull/7"
                .parse::<PullRequestUrl>()
                .unwrap(),
        );
        Review::open(self.tour(steps), &source, state_file).unwrap()
    }
}

fn step(id: &str, pieces: Value) -> Value {
    json!({"id": id, "title": format!("Title of {id}"), "why": "because", "pieces": pieces})
}

fn ids(pieces: &[PieceDiff]) -> Vec<String> {
    pieces.iter().map(|piece| piece.id.to_string()).collect()
}

fn lines(piece: &PieceDiff) -> Vec<String> {
    piece
        .lines()
        .map(|line| {
            let side = match line.location.side {
                Side::Base => "-",
                Side::Head => "+",
            };
            format!("{side}{} {}", line.location.line, line.text)
        })
        .collect()
}

#[test]
fn the_safety_net_comes_after_the_last_step_and_lists_every_change_no_step_covers() {
    let mut review = Shop::with_pull().open(vec![step(
        "order",
        json!([{"file": "app/Order.php", "symbol": "Order::total"}, {"file": "README.md"}]),
    )]);

    review.go_forward();
    assert!(review.go_forward());

    assert_eq!(review.screen(), &Screen::SafetyNet);
    assert_eq!(
        ids(&review.safety_net().unwrap().pieces),
        [
            "app/Order.php#Order::discount",
            "config.yml#hunk-1",
            "config.yml#hunk-2",
            "data.bin",
            "notes/new.txt"
        ]
    );
    assert!(!review.go_forward());
    assert!(review.go_back());
    assert_eq!(review.screen(), &Screen::Step("order".into()));
}

#[test]
fn with_every_change_in_a_step_there_is_no_safety_net() {
    let mut review = Shop::with_pull().open(vec![
        step("order", json!([{"file": "app/Order.php"}])),
        step("rest", json!([{"rest": ""}])),
    ]);
    review.go_to(&"rest".into());

    assert!(review.safety_net().is_none());
    assert!(!review.go_forward());
    review.go_to_safety_net();
    assert_eq!(review.screen(), &Screen::Step("rest".into()));
}

#[test]
fn a_line_range_shows_only_its_lines_and_leaves_the_rest_of_the_file_to_the_safety_net() {
    let mut review = Shop::with_pull().open(vec![
        step("code", json!([{"file": "app/Order.php"}, {"file": "README.md"}, {"file": "data.bin"}, {"file": "notes/new.txt"}])),
        step("config", json!([{"file": "config.yml", "lines": [1, 10]}])),
    ]);
    review.go_to(&"config".into());

    let page = review.current_step().unwrap();
    assert_eq!(ids(&page.pieces), ["config.yml#hunk-1"]);
    assert_eq!(lines(&page.pieces[0]), ["-3 key3: 3", "+3 key3: three"]);
    assert_eq!(
        ids(&review.safety_net().unwrap().pieces),
        ["config.yml#hunk-2"]
    );
}

#[test]
fn a_line_range_that_cuts_through_a_piece_leaves_the_lines_outside_it_to_the_safety_net() {
    let mut review = Shop::with_pull().open(vec![
        step("code", json!([{"file": "README.md"}, {"file": "data.bin"}, {"file": "config.yml"}, {"file": "notes/new.txt"}])),
        step("order", json!([{"file": "app/Order.php", "lines": [1, 10]}])),
    ]);
    review.go_to(&"order".into());

    let page = review.current_step().unwrap();
    assert_eq!(
        ids(&page.pieces),
        [
            "app/Order.php#Order::total",
            "app/Order.php#Order::discount"
        ]
    );
    assert_eq!(
        lines(&page.pieces[1]),
        ["+10     public function discount(): int"]
    );
    let net = review.safety_net().unwrap();
    assert_eq!(ids(&net.pieces), ["app/Order.php#Order::discount"]);
    assert_eq!(
        lines(&net.pieces[0]),
        ["+11     {", "+12         return 0;"]
    );
}

#[test]
fn every_line_links_to_the_same_line_on_github_at_the_tours_commits() {
    let shop = Shop::with_pull();
    let mut review = shop.open(vec![step(
        "order",
        json!([{"file": "config.yml", "symbol": "hunk-1"}, {"file": "notes/new.txt"}]),
    )]);
    review.go_forward();

    let links: Vec<String> = review
        .current_step()
        .unwrap()
        .pieces
        .iter()
        .flat_map(|piece| piece.lines())
        .map(|line| on_github(&review, &line.code_link))
        .collect();
    assert_eq!(
        links,
        [
            format!(
                "https://github.com/acme/shop/blob/{}/config.yml#L3",
                shop.base
            ),
            format!(
                "https://github.com/acme/shop/blob/{}/config.yml#L3",
                shop.head
            ),
            format!(
                "https://github.com/acme/shop/blob/{}/notes/old.txt#L6",
                shop.base
            ),
            format!(
                "https://github.com/acme/shop/blob/{}/notes/new.txt#L6",
                shop.head
            ),
        ]
    );
}

#[test]
fn a_binary_file_is_one_row_with_a_link_to_it_on_github() {
    let shop = Shop::with_pull();
    let mut review = shop.open(vec![step(
        "logo",
        json!([{"file": "data.bin"}, {"file": "README.md"}]),
    )]);
    review.go_forward();

    let page = review.current_step().unwrap();
    let logo = &page.pieces[0];
    assert!(logo.lines().next().is_none());
    assert_eq!(
        logo.binary_file
            .as_ref()
            .map(|link| on_github(&review, link)),
        Some(format!(
            "https://github.com/acme/shop/blob/{}/data.bin",
            shop.head
        ))
    );
    assert!(page.pieces[1].binary_file.is_none());
}

#[test]
fn a_skim_step_shows_folded_until_i_unfold_it() {
    let mut review = Shop::with_pull().open(vec![
        json!({"id": "callers", "title": "Callers", "why": "pass a flag", "skim": true, "pieces": [{"file": "README.md"}]}),
        step("order", json!([{"file": "app/Order.php"}])),
    ]);

    review.go_forward();
    assert!(review.current_step().unwrap().is_folded);
    review.toggle_fold(&"callers".into());
    assert!(!review.current_step().unwrap().is_folded);
    review.toggle_fold(&"callers".into());
    assert!(review.current_step().unwrap().is_folded);

    review.go_forward();
    assert!(!review.current_step().unwrap().is_folded);
}

#[test]
fn a_step_carries_its_check_hint_and_tldr() {
    let mut review = Shop::with_pull().open(vec![json!({
        "id": "order", "title": "Order total", "why": "money",
        "check": "Totals never go negative",
        "tldr": ["total returns 2", "discount is new"],
        "pieces": [{"file": "app/Order.php"}],
    })]);
    review.go_forward();

    let page = review.current_step().unwrap();
    assert_eq!(page.step.check.as_deref(), Some("Totals never go negative"));
    assert_eq!(page.step.tldr, ["total returns 2", "discount is new"]);
}

#[test]
fn a_note_shows_when_the_pr_has_moved_past_the_tours_head() {
    let shop = Shop::with_pull();
    let mut review = shop.open(vec![step("order", json!([{"rest": ""}]))]);
    let sent = SentLog::default();

    review
        .check_pr_head(&FakeGitHub::with_pull(&shop.base, "f00d", &sent))
        .unwrap();

    let note = review.old_head().unwrap();
    assert_eq!(note.tour_head.as_str(), shop.head);
    assert_eq!(note.pr_head.as_str(), "f00d");
    assert_eq!(
        sent.github_calls(),
        [GitHubCall::TipsOf(
            "https://github.com/acme/shop/pull/7".parse().unwrap()
        )]
    );
}

#[test]
fn no_note_while_the_tour_is_on_the_pr_head_or_before_github_answered() {
    let shop = Shop::with_pull();
    let mut review = shop.open(vec![step("order", json!([{"rest": ""}]))]);
    assert!(review.old_head().is_none());

    review
        .check_pr_head(&FakeGitHub::with_pull(
            &shop.base,
            &shop.head,
            &SentLog::default(),
        ))
        .unwrap();

    assert!(review.old_head().is_none());
}
