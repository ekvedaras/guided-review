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

use serde_json::{Value, json};
use support::{FakeGitHub, FixtureRepo, GuidedReview, Run};

const PR: &str = "https://github.com/acme/shop/pull/7";

struct Shop {
    repo: FixtureRepo,
    github: FakeGitHub,
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
        repo.write(
            "app/Services/Pricing.php",
            "<?php\n\nclass Pricing\n{\n    public function discount(): int\n    {\n        return 0;\n    }\n}\n",
        );
        let head = repo.commit("head");
        let github = FakeGitHub::serving_pull("acme/shop", 7, &base, &head);
        Self {
            repo,
            github,
            base,
            head,
        }
    }

    fn tour_path(&self) -> std::path::PathBuf {
        self.repo
            .home()
            .join(".guided-review/tours/acme/shop/7.json")
    }

    fn write_tour(&self, tour: Value) {
        let path = self.tour_path();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, tour.to_string()).unwrap();
    }

    fn tour_on_disk(&self) -> Value {
        serde_json::from_str(&std::fs::read_to_string(self.tour_path()).unwrap()).unwrap()
    }

    fn check(&self) -> Run {
        GuidedReview::new(&self.repo, &self.github).run(&["check", PR])
    }
}

fn step(id: &str, pieces: Value) -> Value {
    json!({"id": id, "title": id, "why": "because", "pieces": pieces})
}

fn tour(steps: Vec<Value>) -> Value {
    json!({"title": "Pricing", "summary": ["Orders get a discount"], "steps": steps})
}

fn covering_everything() -> Value {
    tour(vec![step("all", json!([{"rest": ""}]))])
}

#[test]
fn check_fills_version_pr_base_and_head() {
    let shop = Shop::with_pull();
    shop.write_tour(covering_everything());

    let run = shop.check();

    assert!(run.success, "{}", run.stderr);
    let written = shop.tour_on_disk();
    assert_eq!(written["version"], 1);
    assert_eq!(written["pr"], PR);
    assert_eq!(written["base"], shop.base.as_str());
    assert_eq!(written["head"], shop.head.as_str());
    assert_eq!(written["steps"][0]["pieces"][0]["rest"], "");
}

#[test]
fn unknown_piece_id_is_an_error_and_fails_the_check() {
    let shop = Shop::with_pull();
    shop.write_tour(tour(vec![step(
        "pricing",
        json!([
            {"file": "app/Order.php", "symbol": "Order::nope"},
            {"file": "app/Missing.php"},
            {"rest": ""},
        ]),
    )]));

    let run = shop.check();

    assert!(!run.success);
    assert_eq!(
        run.stdout,
        "\
error: step \"pricing\": unknown piece app/Order.php#Order::nope
error: step \"pricing\": unknown piece app/Missing.php
note: rest \"\" takes 5 pieces in 3 files: README.md, app/Order.php, app/Services/Pricing.php
2 errors, 0 warnings
"
    );
}

#[test]
fn changes_no_step_covers_are_warnings() {
    let shop = Shop::with_pull();
    shop.write_tour(tour(vec![step(
        "order",
        json!([{"file": "app/Order.php", "symbol": "Order::total"}]),
    )]));

    let run = shop.check();

    assert!(run.success, "{}", run.stderr);
    assert_eq!(
        run.stdout,
        "\
warning: no step covers README.md#hunk-1
warning: no step covers app/Services/Pricing.php#(top)
warning: no step covers app/Services/Pricing.php#Pricing
warning: no step covers app/Services/Pricing.php#Pricing::discount
0 errors, 4 warnings
"
    );
}

#[test]
fn a_whole_file_pointer_covers_the_pieces_of_that_file() {
    let shop = Shop::with_pull();
    shop.write_tour(tour(vec![step(
        "all",
        json!([
            {"file": "app/Order.php"},
            {"file": "app/Services/Pricing.php"},
            {"file": "README.md"},
        ]),
    )]));

    assert_eq!(shop.check().stdout, "0 errors, 0 warnings\n");
}

#[test]
fn rest_covers_only_changes_under_its_folder() {
    let shop = Shop::with_pull();
    shop.write_tour(tour(vec![
        step("order", json!([{"file": "app/Order.php"}])),
        step("services", json!([{"rest": "app/Services"}])),
    ]));

    assert_eq!(
        shop.check().stdout,
        "warning: no step covers README.md#hunk-1\nnote: rest \"app/Services\" takes 3 pieces in 1 file: app/Services/Pricing.php\n0 errors, 1 warning\n"
    );
}

#[test]
fn rest_covers_changes_in_steps_nested_anywhere() {
    let shop = Shop::with_pull();
    let mut parent = step("order", json!([{"file": "app/Order.php"}]));
    parent["children"] = json!([step("others", json!([{"rest": "app"}]))]);
    shop.write_tour(tour(vec![
        parent,
        step("docs", json!([{"file": "README.md"}])),
    ]));

    assert_eq!(
        shop.check().stdout,
        "note: rest \"app\" takes 3 pieces in 1 file: app/Services/Pricing.php\n0 errors, 0 warnings\n"
    );
}

#[test]
fn text_rules_are_warnings_not_errors() {
    let shop = Shop::with_pull();
    let mut wordy = step("wordy", json!([{"rest": ""}]));
    wordy["why"] = json!("one\ntwo\nthree");
    let mut crowded = step("crowded", json!([]));
    crowded["pieces"] = json!(vec![json!({"file": "app/Services/Pricing.php"}); 5]);
    let mut steps = vec![wordy, crowded];
    steps.extend((0..6).map(|n| step(&format!("extra-{n}"), json!([]))));
    let mut tour = tour(steps);
    tour["summary"] = json!([
        "one",
        "two",
        "three",
        "this bullet has far too many words in it to be a quick read for anyone"
    ]);
    shop.write_tour(tour);

    let run = shop.check();

    assert!(run.success, "{}", run.stderr);
    assert_eq!(
        run.stdout,
        "\
warning: step \"wordy\": why has 3 lines, keep it to 2
warning: step \"crowded\": 15 pieces, split it into steps of at most 8
warning: summary has 4 bullets, keep it to 3
warning: summary bullet has 16 words, keep it to about 12: this bullet has far too many words in it to be a quick read for anyone
warning: 8 top-level steps, nest details to keep it to 7
note: rest \"\" takes 2 pieces in 2 files: README.md, app/Order.php
0 errors, 5 warnings
"
    );
}

#[test]
fn hunk_pieces_are_valid_pointer_targets_and_cover_their_hunk() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    let lines = |word: &str| {
        (1..=9)
            .map(|n| match n {
                2 | 8 => format!("{word} {n}\n"),
                _ => format!("line {n}\n"),
            })
            .collect::<String>()
    };
    repo.write("ci.yml", &lines("old"));
    let base = repo.commit("base");
    repo.write("ci.yml", &lines("new"));
    let head = repo.commit("head");
    let github = FakeGitHub::serving_pull("acme/shop", 7, &base, &head);
    let path = repo.home().join(".guided-review/tours/acme/shop/7.json");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let write = |pieces: Value| {
        std::fs::write(&path, tour(vec![step("ci", pieces)]).to_string()).unwrap();
        GuidedReview::new(&repo, &github).run(&["check", PR])
    };

    let partial = write(json!([{"file": "ci.yml", "symbol": "hunk-1"}]));
    assert_eq!(
        partial.stdout,
        "warning: no step covers ci.yml#hunk-2\n0 errors, 1 warning\n"
    );

    let unknown = write(json!([{"file": "ci.yml", "symbol": "hunk-3"}]));
    assert!(!unknown.success);

    let both = write(json!([
        {"file": "ci.yml", "symbol": "hunk-1"},
        {"file": "ci.yml", "symbol": "hunk-2"},
    ]));
    assert_eq!(both.stdout, "0 errors, 0 warnings\n");
}

#[test]
fn check_never_moves_a_pinned_tour_when_the_pull_request_has_new_commits() {
    let shop = Shop::with_pull();
    let mut pinned = tour(vec![step(
        "all",
        json!([
            {"file": "app/Order.php"},
            {"file": "app/Services/Pricing.php"},
            {"file": "README.md"},
        ]),
    )]);
    pinned["pr"] = json!(PR);
    pinned["base"] = json!(shop.base);
    pinned["head"] = json!(shop.head);
    shop.write_tour(pinned);
    shop.repo.write("README.md", "hello\nworld\nagain\n");
    let newer_head = shop.repo.commit("newer");
    let github = FakeGitHub::serving_pull("acme/shop", 7, &shop.base, &newer_head);

    let run = GuidedReview::new(&shop.repo, &github).run(&["check", PR]);

    assert!(run.success, "{}", run.stderr);
    assert_eq!(
        run.stdout,
        "warning: PR has new commits; run `guided-review repin`\n0 errors, 0 warnings\n"
    );
    let written = shop.tour_on_disk();
    assert_eq!(written["head"], shop.head.as_str());
    assert_eq!(written["base"], shop.base.as_str());
}

struct CiFile {
    repo: FixtureRepo,
    github: FakeGitHub,
}

impl CiFile {
    fn with_two_changes_in_one_hunk() -> Self {
        let repo = FixtureRepo::cloned_from("acme/shop");
        let lines = |word: &str| {
            (1..=6)
                .map(|n| match n {
                    2 | 3 => format!("{word} {n}\n"),
                    _ => format!("line {n}\n"),
                })
                .collect::<String>()
        };
        repo.write("ci.yml", &lines("old"));
        let base = repo.commit("base");
        repo.write("ci.yml", &lines("new"));
        let head = repo.commit("head");
        let github = FakeGitHub::serving_pull("acme/shop", 7, &base, &head);
        Self { repo, github }
    }

    fn check_with(&self, steps: Vec<Value>) -> Run {
        let path = self
            .repo
            .home()
            .join(".guided-review/tours/acme/shop/7.json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, tour(steps).to_string()).unwrap();
        GuidedReview::new(&self.repo, &self.github).run(&["check", PR])
    }
}

#[test]
fn a_line_range_covers_only_the_changed_lines_inside_it_and_check_warns_about_the_rest() {
    let ci = CiFile::with_two_changes_in_one_hunk();

    let run = ci.check_with(vec![step(
        "ci",
        json!([{"file": "ci.yml", "lines": [1, 2]}]),
    )]);

    assert_eq!(
        run.stdout,
        "warning: 2 changed lines of ci.yml#hunk-1 are in no step\n0 errors, 1 warning\n"
    );
}

#[test]
fn rest_takes_the_changed_lines_a_line_range_left() {
    let ci = CiFile::with_two_changes_in_one_hunk();

    let run = ci.check_with(vec![
        step("ci", json!([{"file": "ci.yml", "lines": [1, 2]}])),
        step("others", json!([{"rest": ""}])),
    ]);

    assert_eq!(
        run.stdout,
        "note: rest \"\" takes 1 piece in 1 file: ci.yml\n0 errors, 0 warnings\n"
    );
}

#[test]
fn line_ranges_cover_a_hunk_together() {
    let ci = CiFile::with_two_changes_in_one_hunk();

    let run = ci.check_with(vec![
        step("first", json!([{"file": "ci.yml", "lines": [1, 2]}])),
        step("second", json!([{"file": "ci.yml", "lines": [3, 6]}])),
    ]);

    assert_eq!(run.stdout, "0 errors, 0 warnings\n");
}

#[test]
fn a_line_range_on_a_file_the_tool_can_parse_warns_to_use_file_and_symbol() {
    let shop = Shop::with_pull();
    shop.write_tour(tour(vec![
        step(
            "order",
            json!([{"file": "app/Order.php", "lines": [1, 10]}]),
        ),
        step("others", json!([{"rest": ""}])),
    ]));

    assert_eq!(
        shop.check().stdout,
        "warning: step \"order\": app/Order.php lines 1-10: the tool can parse this file, use file + symbol\nnote: rest \"\" takes 4 pieces in 2 files: README.md, app/Services/Pricing.php\n0 errors, 1 warning\n"
    );
}

#[test]
fn rest_lists_what_it_took() {
    let shop = Shop::with_pull();
    shop.write_tour(covering_everything());

    assert_eq!(
        shop.check().stdout,
        "note: rest \"\" takes 5 pieces in 3 files: README.md, app/Order.php, app/Services/Pricing.php\n0 errors, 0 warnings\n"
    );
}

#[test]
fn the_tours_dir_override_moves_where_every_command_reads_and_writes_the_tour() {
    let fixture = Shop::with_pull();
    let elsewhere = tempfile::TempDir::new().unwrap();
    let path = elsewhere.path().join("acme/shop/7.json");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, covering_everything().to_string()).unwrap();
    let cli = || {
        GuidedReview::new(&fixture.repo, &fixture.github)
            .with_env("GUIDED_REVIEW_TOURS_DIR", elsewhere.path())
    };

    let check = cli().run(&["check", PR]);
    let tour_path = cli().run(&["tour-path", PR]);

    assert!(check.success, "{}{}", check.stdout, check.stderr);
    let pinned: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(pinned["head"], fixture.head.as_str());
    assert!(!fixture.tour_path().exists());
    assert_eq!(tour_path.stdout.trim(), path.to_str().unwrap());
}
