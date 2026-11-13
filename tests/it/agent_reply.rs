use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::path::Path;
use std::sync::{Arc, Mutex};

use guided_review::app_link::AppFolder;
use guided_review::viewer::{CommentId, HandOffState, ReviewState, ReviewStateFile};

use crate::support::fake_app::{CliEnded, accept_before};
use crate::support::graphql::serve_graphql;
use crate::support::{FakeGitHub, FixtureRepo, GuidedReview};

const PR: &str = "https://github.com/acme/shop/pull/7";
const BUILD: &str = "test-build";
const PENDING: &str = r#"{"data":{"repository":{"pullRequest":{"reviews":{"nodes":[{"id":"REVIEW_7","viewerDidAuthor":true}]}}}}}"#;
const REPLIED: &str = r#"{"data":{"addPullRequestReviewThreadReply":{"comment":{"id":"R1","pullRequestReview":{"id":"REVIEW_7","state":"PENDING"}}}}}"#;

fn state_file(home: &Path) -> ReviewStateFile {
    ReviewStateFile::for_pr(home, &PR.parse().unwrap())
}

fn with_one_hand_off(home: &Path) {
    let state: ReviewState = serde_json::from_value(serde_json::json!({
        "hand_offs": [{
            "id": 0,
            "place": "notes.txt line 10",
            "step": null,
            "mode": "AddToReview",
            "comment": "why ten?",
            "thread": "T1",
            "thread_so_far": [],
            "state": "Delivered",
        }]
    }))
    .unwrap();
    state_file(home).save(&state).unwrap();
}

fn hand_off_state(home: &Path) -> HandOffState {
    state_file(home)
        .load()
        .unwrap()
        .hand_offs()
        .all()
        .first()
        .unwrap()
        .state()
        .clone()
}

fn run_reply(
    repo: &FixtureRepo,
    graphql: Vec<String>,
    id: &str,
) -> (crate::support::Run, Arc<Mutex<Vec<String>>>) {
    run_agent_reply(repo, graphql, &[id, "--body", "Because of the loop."])
}

fn run_agent_reply(
    repo: &FixtureRepo,
    graphql: Vec<String>,
    args: &[&str],
) -> (crate::support::Run, Arc<Mutex<Vec<String>>>) {
    let (url, asked) = serve_graphql(graphql);
    let run = GuidedReview::new(repo, &FakeGitHub { url })
        .with_env("GUIDED_REVIEW_BUILD", Path::new(BUILD))
        .run(&[&["agent-reply", PR], args].concat());
    (run, asked)
}

/// Answers one request line with `answer`, recording the line.
fn fake_app(
    home: &Path,
    cli_ended: &CliEnded,
    answer: &str,
) -> (std::thread::JoinHandle<()>, Arc<Mutex<Vec<String>>>) {
    let cli_ended = cli_ended.clone();
    let socket = AppFolder::in_home(home).socket();
    std::fs::create_dir_all(socket.parent().unwrap()).unwrap();
    let listener = UnixListener::bind(socket).unwrap();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = Arc::clone(&seen);
    let line = format!(r#"{{"version":"{BUILD}","answer":{answer}}}"#);
    let thread = std::thread::spawn(move || {
        let stream = accept_before(
            &listener,
            &cli_ended,
            "the agent-reply request from the CLI",
        );
        let mut request = String::new();
        BufReader::new(&stream).read_line(&mut request).unwrap();
        log.lock().unwrap().push(request);
        (&stream).write_all(format!("{line}\n").as_bytes()).unwrap();
    });
    (thread, seen)
}

#[test]
fn the_reply_is_posted_into_the_pending_review_and_the_state_file_marks_it_answered_when_no_app_runs()
 {
    let repo = FixtureRepo::cloned_from("acme/shop");
    with_one_hand_off(repo.home());

    let (run, asked) = run_reply(
        &repo,
        vec![PENDING.into(), PENDING.into(), REPLIED.into()],
        "h0",
    );

    assert!(run.success, "stderr: {}", run.stderr);
    let asked = asked.lock().unwrap();
    let reply: serde_json::Value = serde_json::from_str(asked.last().unwrap()).unwrap();
    assert_eq!(
        reply["variables"]["input"]["pullRequestReviewThreadId"],
        "T1"
    );
    assert_eq!(
        reply["variables"]["input"]["body"],
        "🤖 Because of the loop."
    );
    assert_eq!(
        reply["variables"]["input"]["pullRequestReviewId"],
        "REVIEW_7"
    );
    assert_eq!(hand_off_state(repo.home()), HandOffState::Answered);
}

#[test]
fn a_running_app_is_told_and_marks_the_hand_off_itself() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    with_one_hand_off(repo.home());
    let cli_ended = CliEnded::default();
    let (app, seen) = fake_app(
        repo.home(),
        &cli_ended,
        &format!(r#"{{"hand_off_answered":{{"pr":"{PR}"}}}}"#),
    );

    let (run, _) = run_reply(
        &repo,
        vec![PENDING.into(), PENDING.into(), REPLIED.into()],
        "h0",
    );

    cli_ended.mark();
    app.join().unwrap();
    assert!(run.success, "stderr: {}", run.stderr);
    let request: serde_json::Value = serde_json::from_str(&seen.lock().unwrap()[0]).unwrap();
    assert_eq!(request["request"]["agent_replied"]["pr"], PR);
    assert_eq!(request["request"]["agent_replied"]["hand_off"], 0);
    assert_eq!(
        hand_off_state(repo.home()),
        HandOffState::Delivered,
        "the app writes the state it holds, so the command leaves the file to it"
    );
}

#[test]
fn an_app_showing_another_pr_leaves_the_marking_to_the_state_file() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    with_one_hand_off(repo.home());
    let cli_ended = CliEnded::default();
    let (app, _) = fake_app(
        repo.home(),
        &cli_ended,
        &format!(r#"{{"not_shown":{{"pr":"{PR}"}}}}"#),
    );

    let (run, _) = run_reply(
        &repo,
        vec![PENDING.into(), PENDING.into(), REPLIED.into()],
        "h0",
    );

    cli_ended.mark();
    app.join().unwrap();
    assert!(run.success, "stderr: {}", run.stderr);
    assert_eq!(hand_off_state(repo.home()), HandOffState::Answered);
}

#[test]
fn an_unknown_hand_off_is_refused_with_the_known_ids_and_nothing_is_posted() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    with_one_hand_off(repo.home());

    let (run, asked) = run_reply(&repo, vec![], "h9");

    assert!(!run.success);
    assert!(
        run.stderr
            .contains("no hand-off h9 on this PR; the known ones are h0"),
        "stderr: {}",
        run.stderr
    );
    assert!(asked.lock().unwrap().is_empty());
    assert_eq!(hand_off_state(repo.home()), HandOffState::Delivered);
}

#[test]
fn a_reply_github_refused_fails_and_leaves_the_hand_off_as_it_was() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    with_one_hand_off(repo.home());
    let gone = r#"{"errors":[{"message":"Could not resolve to a PullRequestReviewThread"}]}"#;

    let (run, _) = run_reply(
        &repo,
        vec![PENDING.into(), PENDING.into(), gone.into()],
        "h0",
    );

    assert!(!run.success);
    assert!(
        run.stderr
            .contains("Could not resolve to a PullRequestReviewThread"),
        "stderr: {}",
        run.stderr
    );
    assert_eq!(hand_off_state(repo.home()), HandOffState::Delivered);
}

#[test]
fn working_on_it_from_the_cli_posts_the_progress_reply_and_the_state_file_keeps_its_id() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    with_one_hand_off(repo.home());

    let (run, asked) = run_agent_reply(
        &repo,
        vec![PENDING.into(), PENDING.into(), REPLIED.into()],
        &["h0", "--working"],
    );

    assert!(run.success, "stderr: {}", run.stderr);
    let asked = asked.lock().unwrap();
    let reply: serde_json::Value = serde_json::from_str(asked.last().unwrap()).unwrap();
    assert_eq!(reply["variables"]["input"]["body"], "🤖 Working on it…");
    assert_eq!(
        hand_off_state(repo.home()),
        HandOffState::WorkingOnIt {
            reply: CommentId::new("R1")
        }
    );
}

#[test]
fn a_body_of_a_dash_is_read_from_stdin_so_the_reply_keeps_its_paragraphs() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    with_one_hand_off(repo.home());
    let (url, asked) = serve_graphql(vec![PENDING.into(), PENDING.into(), REPLIED.into()]);

    let run = GuidedReview::new(&repo, &FakeGitHub { url })
        .with_env("GUIDED_REVIEW_BUILD", Path::new(BUILD))
        .with_stdin("Two jobs:\n\n- price the lines\n- record the discount\n")
        .run(&["agent-reply", PR, "h0", "--body", "-"]);

    assert!(run.success, "stderr: {}", run.stderr);
    let asked = asked.lock().unwrap();
    let reply: serde_json::Value = serde_json::from_str(asked.last().unwrap()).unwrap();
    assert_eq!(
        reply["variables"]["input"]["body"],
        "🤖 Two jobs:\n\n- price the lines\n- record the discount"
    );
}
