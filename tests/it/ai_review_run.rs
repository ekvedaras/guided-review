use crate::support::FixtureRepo;
use guided_review::open_review::open_review;
use guided_review::review_subject::ReviewSubject;
use guided_review::tour::Tour;
use guided_review::viewer::{
    AgentLinkOff, AgentStatus, AiReviewRefusal, AiReviewRun, Review, TerminalId, TourAgentState,
};
use serde_json::json;

const PR: &str = "https://github.com/acme/shop/pull/7";

fn pr() -> ReviewSubject {
    PR.parse().unwrap()
}

fn opened_review() -> (FixtureRepo, String, String, Review) {
    let repo = FixtureRepo::cloned_from("acme/shop");
    repo.write("README.md", "hello\n");
    let base = repo.commit("base");
    repo.write("README.md", "hello\nworld\n");
    let head = repo.commit("head");
    let tour = json!({
        "pr": PR, "base": base, "head": head, "title": "Readme", "summary": [],
        "steps": [{"id": "readme", "title": "Readme", "why": "w", "pieces": [{"file": "README.md"}]}],
    });
    let path = Tour::path_for(&repo.folders(), &pr());
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, tour.to_string()).unwrap();
    let opened = open_review(&pr(), repo.path(), &repo.folders()).unwrap();
    (repo, base, head, opened.review)
}

fn running_review() -> (FixtureRepo, String, String, Review) {
    let (repo, base, head, mut review) = opened_review();
    review.ask_to_run_ai_review().unwrap();
    review.ai_review_agent_started(Ok(TerminalId::new("t9")));
    (repo, base, head, review)
}

fn failure(review: &Review) -> String {
    let AiReviewRun::Failed(reason) = review.ai_review_run() else {
        panic!("failed, got {:?}", review.ai_review_run());
    };
    reason.clone()
}

#[test]
fn a_review_nobody_ran_has_no_run() {
    let (_repo, _, _, review) = opened_review();

    assert_eq!(review.ai_review_run(), &AiReviewRun::NotRun);
    assert_eq!(review.run_ai_review_refusal(), None);
}

#[test]
fn asking_for_a_run_starts_it_and_a_second_ask_is_refused() {
    let (_repo, _, _, mut review) = opened_review();

    review.ask_to_run_ai_review().unwrap();

    assert_eq!(review.ai_review_run(), &AiReviewRun::Starting);
    assert_eq!(
        review.ask_to_run_ai_review(),
        Err(AiReviewRefusal::AlreadyRunning)
    );
}

#[test]
fn a_run_is_refused_while_the_agent_link_is_off() {
    let (_repo, _, _, mut review) = opened_review();
    review.tour_agent_is(&TourAgentState::LinkOff(AgentLinkOff::HcMissing));

    assert_eq!(
        review.ask_to_run_ai_review(),
        Err(AiReviewRefusal::AgentLinkOff)
    );
    assert_eq!(review.ai_review_run(), &AiReviewRun::NotRun);
}

#[test]
fn a_started_agent_is_running_in_its_terminal() {
    let (_repo, _, _, review) = running_review();

    let AiReviewRun::Running(running) = review.ai_review_run() else {
        panic!("running, got {:?}", review.ai_review_run());
    };
    assert_eq!(running.terminal, TerminalId::new("t9"));
    assert!(review.ai_review_run().is_on());
}

#[test]
fn an_agent_that_could_not_start_fails_the_run_with_the_reason() {
    let (_repo, _, _, mut review) = opened_review();
    review.ask_to_run_ai_review().unwrap();

    review.ai_review_agent_started(Err(anyhow::anyhow!("no worktree")));

    assert!(
        failure(&review).contains("could not start"),
        "{}",
        failure(&review)
    );
    assert!(failure(&review).contains("no worktree"));
    assert!(!review.ai_review_run().is_on());
}

#[test]
fn an_agent_idle_before_its_first_turn_is_still_starting_up() {
    let (_repo, _, _, mut review) = running_review();

    review.ai_review_agent_status(AgentStatus::Idle);

    assert!(matches!(review.ai_review_run(), AiReviewRun::Running(_)));
}

#[test]
fn an_agent_idle_after_working_stopped_without_findings() {
    let (_repo, _, _, mut review) = running_review();
    review.ai_review_agent_status(AgentStatus::Working);

    review.ai_review_agent_status(AgentStatus::Idle);

    assert!(failure(&review).contains("stopped without findings"));
}

#[test]
fn an_agent_that_reports_an_error_fails_the_run() {
    let (_repo, _, _, mut review) = running_review();

    review.ai_review_agent_status(AgentStatus::Failed);

    assert!(failure(&review).contains("error"));
}

#[test]
fn a_terminal_that_ended_fails_the_run() {
    let (_repo, _, _, mut review) = running_review();

    review.ai_review_agent_gone();

    assert!(failure(&review).contains("terminal ended"));
}

#[test]
fn the_screen_is_kept_while_running() {
    let (_repo, _, _, mut review) = running_review();

    review.ai_review_agent_screen(vec!["reading".to_owned()]);

    let AiReviewRun::Running(running) = review.ai_review_run() else {
        panic!("running");
    };
    assert_eq!(running.screen, ["reading"]);
}

#[test]
fn a_failed_run_can_be_tried_again() {
    let (_repo, _, _, mut review) = running_review();
    review.ai_review_agent_gone();

    review.ask_to_run_ai_review().unwrap();

    assert_eq!(review.ai_review_run(), &AiReviewRun::Starting);
}

#[test]
fn an_agent_found_again_after_a_restart_that_is_idle_has_stopped() {
    let (_repo, _, _, mut review) = opened_review();
    review.ask_to_run_ai_review().unwrap();

    review.ai_review_agent_resumed(TerminalId::new("t9"), Some(AgentStatus::Idle));

    assert!(failure(&review).contains("stopped without findings"));
}

#[test]
fn an_agent_found_again_after_a_restart_that_works_is_running() {
    let (_repo, _, _, mut review) = opened_review();

    review.ai_review_agent_resumed(TerminalId::new("t9"), Some(AgentStatus::Working));

    assert!(matches!(review.ai_review_run(), AiReviewRun::Running(_)));
}

#[test]
fn reading_the_findings_ends_the_run() {
    let (repo, base, head, mut review) = running_review();
    let path = Tour::findings_path_for(&repo.folders(), &pr());
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let finding =
        json!({"id": "a", "severity": "nit", "body": "Why.", "anchor": {"kind": "change"}});
    std::fs::write(
        path,
        json!({"pr": PR, "base": base, "head": head, "findings": [finding]}).to_string(),
    )
    .unwrap();

    review.read_findings(&repo.folders());

    assert_eq!(review.ai_review_run(), &AiReviewRun::NotRun);
    assert_eq!(review.ai_review().counts().nit, 1);
}

#[test]
fn findings_that_cannot_be_read_fail_the_run() {
    let (repo, _, _, mut review) = running_review();
    let path = Tour::findings_path_for(&repo.folders(), &pr());
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, "not json").unwrap();

    review.read_findings(&repo.folders());

    assert!(failure(&review).contains("findings"));
}

#[test]
fn a_late_status_after_the_findings_changes_nothing() {
    let (repo, base, head, mut review) = running_review();
    let path = Tour::findings_path_for(&repo.folders(), &pr());
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(
        path,
        json!({"pr": PR, "base": base, "head": head, "findings": []}).to_string(),
    )
    .unwrap();
    review.ai_review_agent_status(AgentStatus::Working);
    review.read_findings(&repo.folders());

    review.ai_review_agent_status(AgentStatus::Idle);

    assert_eq!(review.ai_review_run(), &AiReviewRun::NotRun);
}
