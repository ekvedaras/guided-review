use std::path::Path;

use serde_json::{Value, json};

use crate::support::fake_app::{CliEnded, answering_one_request};
use crate::support::{FakeGitHub, FixtureRepo, GuidedReview, Run, folders_in};
use guided_review::viewer::{FixState, ReviewState, ReviewStateFile};

const PR: &str = "https://github.com/acme/shop/pull/7";
const BUILD: &str = "test-build";

fn state_file(home: &Path) -> ReviewStateFile {
    ReviewStateFile::for_subject(&folders_in(home), &PR.parse().unwrap())
}

fn with_a_fix_request(home: &Path) {
    let state: ReviewState = serde_json::from_value(json!({
        "fix_requests": [{
            "finding": {"id": "f1", "severity": "nit", "body": "Rename.", "anchor": {"kind": "change"}},
            "batch": 0,
            "state": "Delivered",
        }]
    }))
    .unwrap();
    state_file(home).save(&state).unwrap();
}

fn fix_state(home: &Path) -> Option<FixState> {
    let state = state_file(home).load().unwrap();
    state
        .fix_requests()
        .state_of(&"f1".to_owned().try_into().unwrap())
        .cloned()
}

fn fix_reply(repo: &FixtureRepo, args: &[&str]) -> Run {
    GuidedReview::new(repo, &FakeGitHub { url: String::new() })
        .with_env("GUIDED_REVIEW_BUILD", Path::new(BUILD))
        .run(&[&["fix-reply", PR, "f1"], args].concat())
}

#[test]
fn with_no_app_running_the_answers_are_kept_in_the_state_file() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    with_a_fix_request(repo.home());

    let working = fix_reply(&repo, &["--working"]);
    assert!(working.success, "{}", working.stderr);
    assert_eq!(fix_state(repo.home()), Some(FixState::Fixing));

    let fixed = fix_reply(&repo, &["--fixed", "abc1234"]);
    assert!(fixed.success, "{}", fixed.stderr);
    assert_eq!(
        fix_state(repo.home()),
        Some(FixState::Fixed {
            commit: "abc1234".to_owned()
        })
    );
}

#[test]
fn a_finding_that_was_not_fixed_keeps_the_reason() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    with_a_fix_request(repo.home());

    let run = fix_reply(&repo, &["--not-fixed", "--body", "It is on purpose."]);

    assert!(run.success, "{}", run.stderr);
    assert_eq!(
        fix_state(repo.home()),
        Some(FixState::NotFixed {
            reason: "It is on purpose.".to_owned()
        })
    );
}

#[test]
fn a_question_waits_for_the_reviewer_in_the_state_file() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    with_a_fix_request(repo.home());

    let run = fix_reply(&repo, &["--question", "--body", "Rename it to what?"]);

    assert!(run.success, "{}", run.stderr);
    assert_eq!(fix_state(repo.home()), Some(FixState::Asked));
}

#[test]
fn a_finding_nobody_asked_to_fix_is_refused() {
    let repo = FixtureRepo::cloned_from("acme/shop");

    let run = fix_reply(&repo, &["--working"]);

    assert!(!run.success);
    assert!(run.stderr.contains("f1"), "{}", run.stderr);
}

#[test]
fn a_running_app_that_shows_the_review_takes_the_answer_and_the_file_is_left_to_it() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    with_a_fix_request(repo.home());
    let cli_ended = CliEnded::default();
    let (app, seen) = answering_one_request(
        repo.home(),
        &cli_ended,
        BUILD,
        &format!(r#"{{"fix_answered":{{"pr":"{PR}"}}}}"#),
    );

    let run = fix_reply(&repo, &["--fixed", "abc1234"]);

    cli_ended.mark();
    app.join().unwrap();
    assert!(run.success, "{}", run.stderr);
    let request: Value = serde_json::from_str(&seen.lock().unwrap()[0]).unwrap();
    assert_eq!(request["request"]["fix_replied"]["pr"], PR);
    assert_eq!(request["request"]["fix_replied"]["finding"], "f1");
    assert_eq!(
        request["request"]["fix_replied"]["reply"],
        json!({"Fixed": {"commit": "abc1234"}})
    );
    assert_eq!(fix_state(repo.home()), Some(FixState::Delivered));
}
