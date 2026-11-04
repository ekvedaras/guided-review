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

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use guided_review::claude_cli::{ClaudeEvalWriter, ClaudeTourJudge};
use guided_review::eval::{EvalWriter, JudgeInput, Rule, Score, TourJudge, WriterBrief};
use guided_review::pull_request::PullRequestUrl;
use tempfile::TempDir;

/// A stand-in `claude -p` that logs its arguments and input, then prints `answer` as a JSON result.
fn fake_claude(dir: &Path, answer: &str) -> PathBuf {
    let script = dir.join("claude");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\necho \"$@\" > \"{dir}/args\"\necho \"$GUIDED_REVIEW_TOURS_DIR\" > \"{dir}/tours_dir\"\ncat > \"{dir}/stdin\"\ncat <<'JSON'\n{answer}\nJSON\n",
            dir = dir.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    script
}

#[test]
fn the_writer_runs_with_narrow_tools_and_gets_the_skill_but_no_note() {
    let dir = TempDir::new().unwrap();
    let writer = ClaudeEvalWriter::with_binary(fake_claude(
        dir.path(),
        r#"{"type":"result","is_error":false,"result":"done"}"#,
    ));
    let pr: PullRequestUrl = "https://github.com/acme/shop/pull/7".parse().unwrap();

    writer
        .write_tour(&WriterBrief {
            pr: &pr,
            skill: "SKILL TEXT",
            tours_dir: Path::new("/runs/one"),
        })
        .unwrap();

    let args = std::fs::read_to_string(dir.path().join("args")).unwrap();
    assert!(!args.contains("dangerously"), "{args}");
    assert!(args.contains("--permission-mode dontAsk"), "{args}");
    assert!(args.contains("--allowedTools"), "{args}");
    assert!(args.contains("Bash(guided-review check:*)"), "{args}");
    assert!(args.contains("--append-system-prompt SKILL TEXT"), "{args}");
    assert!(args.contains("--disallowedTools"), "{args}");
    assert!(args.contains("Write(/runs/one/**)"), "{args}");
    assert!(!args.contains("Write(~/.guided-review/tours"), "{args}");
    let tours_dir = std::fs::read_to_string(dir.path().join("tours_dir")).unwrap();
    assert_eq!(tours_dir.trim(), "/runs/one");
    let stdin = std::fs::read_to_string(dir.path().join("stdin")).unwrap();
    assert!(stdin.contains("https://github.com/acme/shop/pull/7"));
}

#[test]
fn the_writer_failing_is_an_error() {
    let dir = TempDir::new().unwrap();
    let writer = ClaudeEvalWriter::with_binary(fake_claude(
        dir.path(),
        r#"{"type":"result","is_error":true,"result":"out of credit"}"#,
    ));
    let pr: PullRequestUrl = "https://github.com/acme/shop/pull/7".parse().unwrap();

    let error = writer
        .write_tour(&WriterBrief {
            pr: &pr,
            skill: "s",
            tours_dir: Path::new("/runs/one"),
        })
        .unwrap_err();

    assert!(error.to_string().contains("out of credit"));
}

#[test]
fn the_judge_runs_without_tools_and_its_answer_is_read_into_five_verdicts() {
    let dir = TempDir::new().unwrap();
    let answer = r#"{"type":"result","is_error":false,"result":"Here you go:\n```json\n{\"order\":{\"score\":\"pass\",\"reason\":\"flow first\"},\"grouping\":{\"score\":\"partial\",\"reason\":\"tests apart\"},\"diagram\":{\"score\":\"fail\",\"reason\":\"none\"},\"text\":{\"score\":\"pass\",\"reason\":\"why ok\"},\"skim\":{\"score\":\"pass\",\"reason\":\"marked\"}}\n```"}"#;
    let judge = ClaudeTourJudge::with_binary(fake_claude(dir.path(), answer));

    let verdicts = judge
        .judge(&JudgeInput {
            note: "THE NOTE",
            tour_json: "THE TOUR",
        })
        .unwrap();

    assert_eq!(verdicts.get(Rule::Order).unwrap().score, Score::Pass);
    assert_eq!(verdicts.get(Rule::Grouping).unwrap().score, Score::Partial);
    assert_eq!(verdicts.get(Rule::Diagram).unwrap().reason, "none");
    let args = std::fs::read_to_string(dir.path().join("args")).unwrap();
    assert!(args.contains("--tools"), "{args}");
    assert!(!args.contains("dangerously"), "{args}");
    let stdin = std::fs::read_to_string(dir.path().join("stdin")).unwrap();
    assert!(stdin.contains("THE NOTE") && stdin.contains("THE TOUR"));
}
