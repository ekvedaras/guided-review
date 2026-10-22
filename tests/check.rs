#[allow(dead_code)]
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
        "warning: no step covers README.md#hunk-1\n0 errors, 1 warning\n"
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

    assert_eq!(shop.check().stdout, "0 errors, 0 warnings\n");
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
