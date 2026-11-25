use std::path::Path;
use std::sync::Arc;

use crate::support::fake_checkouts::FakeCheckouts;
use crate::support::fake_tour_agent::{FakeTourAgent, TourAgentCall};
use crate::support::{FixtureRepo, folders_in};
use guided_review::build::Build;
use guided_review::open_review::open_review;
use guided_review::review_subject::ReviewSubject;
use guided_review::tour::Tour;
use guided_review::viewer::{
    AgentLinkOff, AgentStatus, AiReviewAction, AiReviewRefusal, AiReviewRun, Effect, Intent,
    PageRow, PaletteAction, Refusal, Review, ReviewAgents, TerminalId, TourAgentPort,
    TourAgentState, WindowMoment,
};
use serde_json::json;
use tempfile::TempDir;

const PR: &str = "https://github.com/acme/shop/pull/7";

fn pr() -> ReviewSubject {
    PR.parse().unwrap()
}

struct Desk {
    repo: FixtureRepo,
    base: String,
    head: String,
    review: Review,
    fake: Arc<FakeTourAgent>,
    home: TempDir,
}

impl Desk {
    fn new() -> Self {
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
        let review = open_review(&pr(), repo.path(), &repo.folders())
            .unwrap()
            .review;
        Self {
            repo,
            base,
            head,
            review,
            fake: Arc::new(FakeTourAgent::new()),
            home: TempDir::new().unwrap(),
        }
    }

    fn agents(&self) -> ReviewAgents {
        ReviewAgents::new(
            Arc::clone(&self.fake) as Arc<dyn TourAgentPort>,
            Arc::new(FakeCheckouts::default().clean_at("/clones/shop", "abc")),
            folders_in(self.home.path()),
        )
    }

    /// What the app does for Run AI review and Try again.
    fn run(&mut self) {
        self.review.ask_to_run_ai_review().unwrap();
        let started = self.agents().start(
            &pr(),
            None,
            Path::new("/clones/shop"),
            self.review.head(),
            &pr(),
        );
        self.review.ai_review_agent_started(started);
    }

    /// What the agent leaves, and what the app does when it hears findings-done.
    fn findings_done(&mut self, findings: &[serde_json::Value]) {
        let path = Tour::findings_path_for(&self.repo.folders(), &pr());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let file = json!({"pr": PR, "base": self.base, "head": self.head, "findings": findings});
        std::fs::write(path, file.to_string()).unwrap();
        self.review.findings_done(&self.repo.folders());
    }
}

/// The button the summary screen's toolbar offers.
fn toolbar_button(review: &Review) -> Option<AiReviewAction> {
    review.page().rows().iter().find_map(|row| match row {
        PageRow::Toolbar(toolbar) => Some(toolbar.run_ai_review),
        _ => None,
    })?
}

fn a_nit() -> serde_json::Value {
    json!({"id": "a", "severity": "nit", "body": "Why.", "anchor": {"kind": "change"}})
}

#[test]
fn a_run_starts_the_review_agent_and_follows_it() {
    let mut desk = Desk::new();

    desk.run();

    assert!(matches!(
        desk.review.ai_review_run(),
        AiReviewRun::Running(_)
    ));
    assert_eq!(desk.fake.review_agents_of(PR).len(), 1);
}

#[test]
fn an_agent_that_goes_idle_without_findings_fails_the_run_and_keeps_its_terminal_to_answer() {
    let mut desk = Desk::new();
    desk.run();

    desk.review.ai_review_agent_status(AgentStatus::Working);
    desk.review.ai_review_agent_status(AgentStatus::Idle);

    assert!(matches!(
        desk.review.ai_review_run(),
        AiReviewRun::Failed { .. }
    ));
    assert_eq!(desk.fake.review_agents_of(PR).len(), 1);
}

#[test]
fn findings_done_clears_the_run_and_leaves_the_agent_idle_to_talk_to() {
    let mut desk = Desk::new();
    desk.run();
    desk.review.ai_review_agent_status(AgentStatus::Working);

    desk.findings_done(&[a_nit()]);

    assert_eq!(desk.review.ai_review_run(), &AiReviewRun::NotRun);
    assert_eq!(desk.review.ai_review().counts().nit, 1);
    assert_eq!(desk.fake.review_agents_of(PR).len(), 1);
}

#[test]
fn an_idle_agent_found_again_after_findings_were_read_is_not_a_failed_run() {
    let mut desk = Desk::new();
    desk.run();
    desk.findings_done(&[a_nit()]);

    desk.review
        .ai_review_agent_resumed(TerminalId::new("t1"), Some(AgentStatus::Idle));

    assert_eq!(desk.review.ai_review_run(), &AiReviewRun::NotRun);
}

#[test]
fn a_second_run_that_went_idle_without_findings_done_is_a_failed_run_when_the_review_opens_again() {
    let mut desk = Desk::new();
    desk.run();
    desk.findings_done(&[a_nit()]);
    desk.run();
    let mut reopened = open_review(&pr(), desk.repo.path(), &desk.repo.folders())
        .unwrap()
        .review;

    reopened.ai_review_agent_resumed(TerminalId::new("t1"), Some(AgentStatus::Idle));

    assert!(
        matches!(reopened.ai_review_run(), AiReviewRun::Failed { .. }),
        "{:?}",
        reopened.ai_review_run()
    );
}

#[test]
fn a_run_whose_terminal_is_gone_does_not_offer_show_agent_but_a_stopped_agent_does() {
    let mut desk = Desk::new();
    desk.run();
    desk.review.ai_review_agent_status(AgentStatus::Working);
    desk.review.ai_review_agent_status(AgentStatus::Idle);
    assert!(desk.review.ai_review_stage().can_show_agent);

    desk.review.ask_to_run_ai_review().unwrap();
    desk.review.ai_review_agent_gone();

    assert!(!desk.review.ai_review_stage().can_show_agent);
}

#[test]
fn trying_again_after_a_failure_replaces_the_agent() {
    let mut desk = Desk::new();
    desk.run();
    desk.review.ai_review_agent_status(AgentStatus::Working);
    desk.review.ai_review_agent_status(AgentStatus::Idle);

    desk.run();

    assert!(matches!(
        desk.review.ai_review_run(),
        AiReviewRun::Running(_)
    ));
    assert_eq!(desk.fake.review_agents_of(PR).len(), 1);
    assert!(
        desk.fake
            .calls()
            .iter()
            .any(|call| matches!(call, TourAgentCall::Kill(_)))
    );
}

#[test]
fn an_agent_that_could_not_start_fails_the_run() {
    let mut desk = Desk::new();
    desk.fake = Arc::new(FakeTourAgent::new().failing_to_start("herdr refused"));

    desk.run();

    let AiReviewRun::Failed { reason, .. } = desk.review.ai_review_run() else {
        panic!("failed, got {:?}", desk.review.ai_review_run());
    };
    assert!(reason.contains("herdr refused"), "{reason}");
}

#[test]
fn the_sidebar_offers_run_before_a_run_and_run_again_after_findings() {
    let mut desk = Desk::new();

    let stage = desk.review.ai_review_stage();
    assert_eq!(stage.line, "Not run");
    assert_eq!(stage.action, Some(AiReviewAction::Run));
    assert!(!stage.can_show_agent);
    assert!(!stage.is_failure);

    desk.findings_done(&[a_nit()]);

    let stage = desk.review.ai_review_stage();
    assert_eq!(stage.line, "1 nit");
    assert_eq!(stage.action, Some(AiReviewAction::RunAgain));
}

#[test]
fn the_sidebar_says_starting_then_running_with_show_agent_and_no_button() {
    let mut desk = Desk::new();
    desk.review.ask_to_run_ai_review().unwrap();

    let starting = desk.review.ai_review_stage();
    assert!(starting.line.contains("Starting"), "{}", starting.line);
    assert_eq!(starting.action, None);
    assert!(!starting.can_show_agent);

    desk.review
        .ai_review_agent_started(Ok(TerminalId::new("t9")));
    let running = desk.review.ai_review_stage();
    assert_eq!(running.action, None);
    assert!(running.can_show_agent);
    assert!(!running.is_failure);
}

#[test]
fn the_sidebar_says_why_a_run_failed_and_offers_try_again() {
    let mut desk = Desk::new();
    desk.run();
    desk.review.ai_review_agent_gone();

    let stage = desk.review.ai_review_stage();

    assert!(stage.is_failure);
    assert!(stage.line.contains("terminal ended"), "{}", stage.line);
    assert_eq!(stage.action, Some(AiReviewAction::TryAgain));
}

#[test]
fn with_the_agent_link_off_no_run_is_offered() {
    let mut desk = Desk::new();
    desk.review
        .tour_agent_is(&TourAgentState::LinkOff(AgentLinkOff::HcMissing));

    assert_eq!(desk.review.ai_review_stage().action, None);
    assert_eq!(toolbar_button(&desk.review), None);
    assert!(
        !desk
            .review
            .palette_actions(Build::Local)
            .contains(&PaletteAction::RunAiReview)
    );
}

#[test]
fn the_palette_the_overview_and_the_sidebar_start_the_run_through_the_same_intent() {
    let mut desk = Desk::new();

    assert!(
        desk.review
            .palette_actions(Build::Local)
            .contains(&PaletteAction::RunAiReview)
    );
    assert_eq!(PaletteAction::RunAiReview.label(), "Run AI review");
    assert_eq!(
        Intent::from(PaletteAction::RunAiReview),
        Intent::RunAiReview
    );
    assert_eq!(toolbar_button(&desk.review), Some(AiReviewAction::Run));
    assert_eq!(
        desk.review
            .decide(Intent::RunAiReview, WindowMoment::default()),
        Effect::RunAiReview
    );

    desk.review.ask_to_run_ai_review().unwrap();

    assert_eq!(toolbar_button(&desk.review), None);
    assert!(
        !desk
            .review
            .palette_actions(Build::Local)
            .contains(&PaletteAction::RunAiReview)
    );
    assert_eq!(
        desk.review
            .decide(Intent::RunAiReview, WindowMoment::default()),
        Effect::Refused(Refusal::AiReview(AiReviewRefusal::AlreadyRunning))
    );
}

#[test]
fn the_buttons_are_named_for_what_they_do() {
    assert_eq!(AiReviewAction::Run.label(), "Run AI review");
    assert_eq!(AiReviewAction::RunAgain.label(), "Run again");
    assert_eq!(AiReviewAction::TryAgain.label(), "Try again");
}
