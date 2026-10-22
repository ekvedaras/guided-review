#[allow(dead_code)]
mod support;

use serde_json::{Value, json};
use support::{FakeGitHub, FixtureRepo, GuidedReview, Run};

const PR: &str = "https://github.com/acme/shop/pull/7";

fn order_php(total: u32, tax: u32, keep: u32) -> String {
    format!(
        "<?php\n\nclass Order\n{{\n    public function total(): int\n    {{\n        return {total};\n    }}\n\n    public function tax(): int\n    {{\n        return {tax};\n    }}\n\n    public function keep(): int\n    {{\n        return {keep};\n    }}\n}}\n"
    )
}

fn yaml(lines: &[(usize, &str)]) -> String {
    (1..=30)
        .map(|number| {
            let text = lines
                .iter()
                .find(|(at, _)| *at == number)
                .map_or("same", |(_, text)| text);
            format!("key{number}: {text}\n")
        })
        .collect()
}

struct Shop {
    repo: FixtureRepo,
    base: String,
    head: String,
}

impl Shop {
    /// A tour pinned to `head`, which changes total, tax and keep and nothing else.
    fn with_tour_at_first_head() -> Self {
        let repo = FixtureRepo::cloned_from("acme/shop");
        repo.write("app/Order.php", &order_php(1, 1, 1));
        repo.write("config.yaml", &yaml(&[]));
        let base = repo.commit("base");
        repo.write("app/Order.php", &order_php(2, 5, 9));
        repo.write("config.yaml", &yaml(&[(15, "first")]));
        let head = repo.commit("head");
        let shop = Self { repo, base, head };
        shop.write_tour(json!({
            "title": "Orders",
            "summary": ["Orders change"],
            "steps": [{"id": "orders", "title": "Orders", "why": "because", "pieces": [
                {"file": "app/Order.php", "symbol": "Order::total"},
                {"file": "app/Order.php", "symbol": "Order::tax"},
                {"file": "app/Order.php", "symbol": "Order::keep"},
                {"file": "config.yaml", "symbol": "hunk-1"},
            ]}],
        }));
        shop.check();
        shop
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
        let github = FakeGitHub::serving_pull("acme/shop", 7, &self.base, &self.head);
        GuidedReview::new(&self.repo, &github).run(&["check", PR])
    }

    fn repin_onto(&self, base: &str, head: &str) -> Run {
        let github = FakeGitHub::serving_pull("acme/shop", 7, base, head);
        GuidedReview::new(&self.repo, &github).run(&["repin", PR])
    }

    fn pointers(&self) -> Vec<Value> {
        self.tour_on_disk()["steps"][0]["pieces"]
            .as_array()
            .unwrap()
            .clone()
    }
}

#[test]
fn every_piece_is_same_changed_gone_or_new() {
    let shop = Shop::with_tour_at_first_head();
    shop.repo.write("app/Order.php", &order_php(3, 1, 9));
    shop.repo.write(
        "app/Pricing.php",
        "<?php\n\nclass Pricing\n{\n    public function discount(): int\n    {\n        return 0;\n    }\n}\n",
    );
    let second_head = shop.repo.commit("second head");

    let run = shop.repin_onto(&shop.base, &second_head);

    assert_eq!(
        run.stdout_with(&[]),
        "\
changed app/Order.php#Order::total
same app/Order.php#Order::keep
new app/Pricing.php#(top)
new app/Pricing.php#Pricing
new app/Pricing.php#Pricing::discount
same config.yaml#hunk-1
gone app/Order.php#Order::tax
2 same, 1 changed, 1 gone, 3 new
"
    );
}

#[test]
fn tour_file_moves_to_the_new_head_in_place_and_keeps_no_copies() {
    let shop = Shop::with_tour_at_first_head();
    shop.repo.write("app/Order.php", &order_php(3, 5, 9));
    let second_head = shop.repo.commit("second head");

    let run = shop.repin_onto(&shop.base, &second_head);

    assert!(run.success, "{}", run.stderr);
    let tour = shop.tour_on_disk();
    assert_eq!(tour["head"], second_head.as_str());
    assert_eq!(tour["previous_head"], shop.head.as_str());
    assert_eq!(tour["base"], shop.base.as_str());
    assert_eq!(tour["title"], "Orders");
    let files: Vec<_> = std::fs::read_dir(shop.tour_path().parent().unwrap())
        .unwrap()
        .collect();
    assert_eq!(files.len(), 1);
}

#[test]
fn a_rebase_with_no_real_change_has_no_changed_pieces() {
    let shop = Shop::with_tour_at_first_head();
    shop.repo.branch_from("rebased", &shop.base);
    shop.repo.write("unrelated.txt", "new on main\n");
    let new_base = shop.repo.commit("main moved on");
    shop.repo.write("app/Order.php", &order_php(2, 5, 9));
    shop.repo.write("config.yaml", &yaml(&[(15, "first")]));
    let rebased_head = shop.repo.commit("head again");

    let run = shop.repin_onto(&new_base, &rebased_head);

    let stdout = run.stdout_with(&[]);
    assert!(
        stdout.ends_with("4 same, 0 changed, 0 gone, 0 new\n"),
        "{stdout}"
    );
    let tour = shop.tour_on_disk();
    assert_eq!(tour["base"], new_base.as_str());
    assert_eq!(tour["head"], rebased_head.as_str());
}

#[test]
fn an_unchanged_hunk_stays_same_when_a_hunk_is_added_above_it() {
    let shop = Shop::with_tour_at_first_head();
    shop.repo
        .write("config.yaml", &yaml(&[(3, "added above"), (15, "first")]));
    let second_head = shop.repo.commit("second head");

    let run = shop.repin_onto(&shop.base, &second_head);

    let stdout = run.stdout_with(&[]);
    assert!(stdout.contains("same config.yaml#hunk-2\n"), "{stdout}");
    assert!(stdout.contains("new config.yaml#hunk-1\n"), "{stdout}");
    assert!(!stdout.contains("gone config.yaml"), "{stdout}");
    assert_eq!(
        shop.pointers().last().unwrap(),
        &json!({"file": "config.yaml", "symbol": "hunk-2"})
    );
}

#[test]
fn a_hunk_edited_in_place_is_changed_not_gone_and_new() {
    let shop = Shop::with_tour_at_first_head();
    shop.repo
        .write("config.yaml", &yaml(&[(15, "edited again")]));
    let second_head = shop.repo.commit("second head");

    let run = shop.repin_onto(&shop.base, &second_head);

    let stdout = run.stdout_with(&[]);
    assert!(stdout.contains("changed config.yaml#hunk-1\n"), "{stdout}");
    assert!(!stdout.contains("gone config.yaml"), "{stdout}");
}

#[test]
fn repin_stores_the_piece_statuses_in_the_review_state_for_the_app() {
    let shop = Shop::with_tour_at_first_head();
    shop.repo.write("app/Order.php", &order_php(3, 1, 9));
    let second_head = shop.repo.commit("second head");

    let run = shop.repin_onto(&shop.base, &second_head);

    assert!(run.success, "{}", run.stderr);
    let state_path = shop
        .repo
        .home()
        .join(".guided-review/state/acme/shop/7.json");
    let state: Value = serde_json::from_str(&std::fs::read_to_string(state_path).unwrap()).unwrap();
    let record = &state["last_repin"];
    assert_eq!(record["from_head"], shop.head.as_str());
    assert_eq!(record["to_head"], second_head.as_str());
    assert_eq!(record["pieces"]["app/Order.php#Order::total"], "changed");
    assert_eq!(
        record["gone_by_step"],
        json!({"orders": ["app/Order.php#Order::tax"]})
    );
}

#[test]
fn repin_without_a_tour_file_fails() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    let base = repo.commit("base");
    let github = FakeGitHub::serving_pull("acme/shop", 7, &base, &base);

    let run = GuidedReview::new(&repo, &github).run(&["repin", PR]);

    assert!(!run.success);
    assert!(
        run.stderr.contains("could not read the tour file"),
        "{}",
        run.stderr
    );
}

#[test]
fn a_line_range_in_a_file_that_changed_gets_a_warning_and_one_in_an_untouched_file_does_not() {
    let shop = Shop::with_tour_at_first_head();
    shop.write_tour(json!({
        "title": "Orders",
        "summary": ["Orders change"],
        "steps": [{"id": "orders", "title": "Orders", "why": "because", "pieces": [
            {"file": "config.yaml", "lines": [14, 16]},
            {"file": "app/Order.php", "lines": [5, 9]},
        ]}],
    }));
    shop.check();
    shop.repo
        .write("config.yaml", &yaml(&[(3, "added above"), (15, "first")]));
    let second_head = shop.repo.commit("second head");

    let run = shop.repin_onto(&shop.base, &second_head);

    let stdout = run.stdout_with(&[]);
    assert!(
        stdout.contains("warning: config.yaml lines 14-16 may have moved; check the range\n"),
        "{stdout}"
    );
    assert!(!stdout.contains("warning: app/Order.php"), "{stdout}");
}
