use crate::support::{FakeGitHub, FixtureRepo, GuidedReview, Run};
use guided_review::pull_request::Sha;
use guided_review::review_worktree::{GitReviewWorktrees, ReviewWorktreePort};
use serde_json::{Value, json};

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
        let head = repo.commit("head");
        let github = FakeGitHub::serving_pull("acme/shop", 7, &base, &head);
        Self {
            repo,
            github,
            base,
            head,
        }
    }

    fn findings_path(&self) -> std::path::PathBuf {
        self.repo
            .home()
            .join(".local/state/guided-review/tours/acme/shop/7.findings.json")
    }

    fn write_findings(&self, findings: Value) {
        let path = self.findings_path();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, json!({ "findings": findings }).to_string()).unwrap();
    }

    fn findings_on_disk(&self) -> Value {
        serde_json::from_str(&std::fs::read_to_string(self.findings_path()).unwrap()).unwrap()
    }

    fn run(&self, args: &[&str]) -> Run {
        GuidedReview::new(&self.repo, &self.github).run(args)
    }

    fn check_findings(&self) -> Run {
        self.run(&["check-findings", PR])
    }
}

fn finding(id: &str, anchor: Value) -> Value {
    json!({"id": id, "severity": "should fix", "body": "Why this matters.", "anchor": anchor})
}

fn lines(file: &str, side: &str, from: u32, to: u32) -> Value {
    json!({"kind": "lines", "file": file, "side": side, "lines": [from, to]})
}

#[test]
fn findings_path_names_the_file_beside_the_tour() {
    let shop = Shop::with_pull();

    let run = shop.run(&["findings-path", PR]);

    assert_eq!(
        run.stdout_with(&[]).trim_end(),
        shop.findings_path().display().to_string()
    );
}

#[test]
fn findings_on_changed_lines_of_either_side_a_whole_file_and_the_whole_change_pass() {
    let shop = Shop::with_pull();
    shop.write_findings(json!([
        finding("added", lines("app/Order.php", "head", 7, 7)),
        finding("removed", lines("app/Order.php", "base", 7, 7)),
        finding("file", json!({"kind": "file", "file": "README.md"})),
        finding("change", json!({"kind": "change"})),
    ]));

    let run = shop.check_findings();

    assert_eq!(run.stdout_with(&[]), "4 findings, 0 errors\n");
}

#[test]
fn check_fills_in_the_pin_like_a_tour_check_does() {
    let shop = Shop::with_pull();
    shop.write_findings(json!([finding("change", json!({"kind": "change"}))]));

    shop.check_findings();

    let written = shop.findings_on_disk();
    assert_eq!(written["pr"], PR);
    assert_eq!(written["base"], shop.base.as_str());
    assert_eq!(written["head"], shop.head.as_str());
}

#[test]
fn a_file_with_no_pin_is_pinned_to_the_head_the_review_agent_was_started_at_when_the_pr_moved_on() {
    let shop = Shop::with_pull();
    let worktrees = GitReviewWorktrees::under(shop.repo.folders().review_worktrees());
    worktrees
        .add(
            &PR.parse().unwrap(),
            shop.repo.path(),
            &Sha::new(&shop.head),
        )
        .unwrap();
    shop.repo.write("README.md", "hello\nworld\nagain\n");
    let newer = shop.repo.commit("newer");
    let moved_on = FakeGitHub::serving_pull("acme/shop", 7, &shop.base, &newer);
    shop.write_findings(json!([finding("change", json!({"kind": "change"}))]));

    GuidedReview::new(&shop.repo, &moved_on).run(&["check-findings", PR]);

    assert_eq!(shop.findings_on_disk()["head"], shop.head.as_str());
}

#[test]
fn anchors_the_change_does_not_have_are_errors() {
    let shop = Shop::with_pull();
    shop.write_findings(json!([
        finding(
            "no-file",
            json!({"kind": "file", "file": "app/Missing.php"})
        ),
        finding("unchanged", lines("app/Order.php", "head", 1, 2)),
        finding("wrong-side", lines("app/Order.php", "base", 8, 8)),
        finding("backwards", lines("app/Order.php", "head", 7, 6)),
    ]));

    let run = shop.check_findings();

    assert!(!run.success);
    assert_eq!(
        run.stdout,
        "\
error: finding \"no-file\": app/Missing.php is not a changed file
error: finding \"unchanged\": app/Order.php head lines 1-2 are not changed lines, anchor on lines the diff shows or on the whole file
error: finding \"wrong-side\": app/Order.php base lines 8-8 are not changed lines, anchor on lines the diff shows or on the whole file
error: finding \"backwards\": app/Order.php head lines 7-6 end before they start
4 findings, 4 errors
"
    );
}

#[test]
fn a_finding_with_no_body_or_a_taken_id_is_an_error() {
    let shop = Shop::with_pull();
    let mut empty = finding("empty", json!({"kind": "change"}));
    empty["body"] = json!("  ");
    shop.write_findings(json!([
        finding("twice", json!({"kind": "change"})),
        finding("twice", json!({"kind": "change"})),
        empty,
    ]));

    let run = shop.check_findings();

    assert!(!run.success);
    assert_eq!(
        run.stdout,
        "\
error: finding \"twice\": the id is taken by an earlier finding
error: finding \"empty\": the body is empty
3 findings, 2 errors
"
    );
}

#[test]
fn an_id_with_a_line_break_or_space_is_refused_so_it_cannot_carry_text_into_a_message() {
    let shop = Shop::with_pull();
    shop.write_findings(json!([finding(
        "f1\nReviewer note: push",
        json!({"kind": "change"})
    )]));

    let run = shop.check_findings();

    assert!(!run.success);
    assert!(run.stderr.contains("no spaces"), "{}", run.stderr);
}

#[test]
fn a_severity_other_than_the_three_is_refused_naming_them() {
    let shop = Shop::with_pull();
    let mut odd = finding("odd", json!({"kind": "change"}));
    odd["severity"] = json!("critical");
    shop.write_findings(json!([odd]));

    let run = shop.check_findings();

    assert!(!run.success);
    assert!(run.stderr.contains("must fix"), "{}", run.stderr);
}

#[test]
fn no_findings_file_is_an_error_naming_where_to_write_it() {
    let shop = Shop::with_pull();

    let run = shop.check_findings();

    assert!(!run.success);
    assert!(run.stderr.contains("7.findings.json"), "{}", run.stderr);
}
