use std::path::Path;

use serde_json::{Value, json};

use crate::support::fake_app::{CliEnded, answering_one_request};
use crate::support::{FakeGitHub, FixtureRepo, GuidedReview, Run};

const PR: &str = "https://github.com/acme/shop/pull/7";
const BUILD: &str = "test-build";
const SHA: &str = "0123456789abcdef0123456789abcdef01234567";

fn write_findings(repo: &FixtureRepo, file: Value) {
    let path = repo
        .home()
        .join(".local/state/guided-review/tours/acme/shop/7.findings.json");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, file.to_string()).unwrap();
}

fn pinned() -> Value {
    json!({
        "pr": PR,
        "base": SHA,
        "head": SHA,
        "findings": [{"id": "f1", "severity": "nit", "body": "Rename.", "anchor": {"kind": "change"}}],
    })
}

fn findings_done(repo: &FixtureRepo) -> Run {
    GuidedReview::new(repo, &FakeGitHub { url: String::new() })
        .with_env("GUIDED_REVIEW_BUILD", Path::new(BUILD))
        .run(&["findings-done", PR])
}

#[test]
fn a_running_app_is_told_to_read_the_findings() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    write_findings(&repo, pinned());
    let cli_ended = CliEnded::default();
    let (app, seen) = answering_one_request(
        repo.home(),
        &cli_ended,
        BUILD,
        &format!(r#"{{"findings_read":{{"pr":"{PR}","count":1}}}}"#),
    );

    let run = findings_done(&repo);

    cli_ended.mark();
    app.join().unwrap();
    assert!(run.success, "{}", run.stderr);
    let request: Value = serde_json::from_str(&seen.lock().unwrap()[0]).unwrap();
    assert_eq!(request["request"]["findings_done"]["pr"], PR);
    assert_eq!(
        run.stdout,
        format!("guided-review app read 1 finding of {PR}\n")
    );
}

#[test]
fn with_no_app_running_the_findings_wait_on_disk_for_the_next_open() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    write_findings(&repo, pinned());

    let run = findings_done(&repo);

    assert!(run.success, "{}", run.stderr);
    assert!(run.stdout.contains("no app is running"), "{}", run.stdout);
}

#[test]
fn an_app_showing_another_review_leaves_the_findings_on_disk() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    write_findings(&repo, pinned());
    let cli_ended = CliEnded::default();
    let (app, _) = answering_one_request(
        repo.home(),
        &cli_ended,
        BUILD,
        &format!(r#"{{"not_shown":{{"pr":"{PR}"}}}}"#),
    );

    let run = findings_done(&repo);

    cli_ended.mark();
    app.join().unwrap();
    assert!(run.success, "{}", run.stderr);
    assert!(run.stdout.contains("does not show"), "{}", run.stdout);
}

#[test]
fn findings_that_check_findings_has_not_pinned_are_refused() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    write_findings(&repo, json!({"findings": []}));

    let run = findings_done(&repo);

    assert!(!run.success);
    assert!(run.stderr.contains("check-findings"), "{}", run.stderr);
}

#[test]
fn no_findings_file_is_refused() {
    let repo = FixtureRepo::cloned_from("acme/shop");

    let run = findings_done(&repo);

    assert!(!run.success);
    assert!(run.stderr.contains("7.findings.json"), "{}", run.stderr);
}
