use std::sync::Arc;

use crate::support::fake_tour_agent::FakeTourAgent;
use guided_review::pull_request::PullRequestUrl;
use guided_review::viewer::{
    ActivePr, AgentStatus, Finished, OpenDecision, OpenOrigin, TerminalId, TourAgentPort,
    TourAgents, TourFact, WriteDecision, WritingStage,
};

const SHOWN: &str = "https://github.com/acme/shop/pull/7";
const NO_TOUR: &str = "https://github.com/acme/shop/pull/9";
const ELSEWHERE: &str = "https://github.com/acme/shop/pull/10";

fn pr(url: &str) -> PullRequestUrl {
    url.parse().unwrap()
}

fn showing_a_pr() -> ActivePr {
    let mut active = ActivePr::default();
    let OpenDecision::Build(ticket) = active.request(pr(SHOWN), OpenOrigin::Reviewer) else {
        panic!("the first open builds");
    };
    let Finished::Switch(()) = active.finished(ticket, Ok(())) else {
        panic!("the first build shows");
    };
    active
}

fn agents(fake: &Arc<FakeTourAgent>) -> TourAgents {
    TourAgents::new(Arc::clone(fake) as Arc<dyn TourAgentPort>).starting_in("/clones/shop".into())
}

/// Picks the PR with no tour, as the switcher does, and starts its writer through the fake herdr.
fn pick_no_tour(active: &mut ActivePr, fake: &Arc<FakeTourAgent>) -> TerminalId {
    assert_eq!(active.write(pr(NO_TOUR)), WriteDecision::StartWriter);
    let started = agents(fake).write_tour(&pr(NO_TOUR), &pr(SHOWN));
    let terminal = started.as_ref().unwrap().clone();
    active.writer_started(&pr(NO_TOUR), started);
    terminal
}

fn loading_stage(active: &ActivePr) -> WritingStage {
    let (writing_for, writing) = active.loading().expect("the loading screen shows");
    assert_eq!(writing_for, &pr(NO_TOUR));
    writing.stage.clone()
}

#[test]
fn picking_a_pr_with_no_tour_shows_its_writer_on_the_loading_screen() {
    let fake = Arc::new(FakeTourAgent::new());
    let mut active = showing_a_pr();
    assert!(active.loading().is_none());

    let terminal = pick_no_tour(&mut active, &fake);

    assert_eq!(fake.agents_of(NO_TOUR), [terminal.to_string()]);
    let (_, writing) = active.loading().unwrap();
    assert_eq!(writing.stage, WritingStage::Writing);
    assert_eq!(writing.terminal.as_ref(), Some(&terminal));
    assert_eq!(
        active.shown(),
        Some(&pr(SHOWN)),
        "the old PR stays behind it"
    );
}

#[test]
fn the_loading_screen_follows_the_writers_status_and_screen_lines() {
    let fake = Arc::new(FakeTourAgent::new());
    let mut active = showing_a_pr();
    pick_no_tour(&mut active, &fake);

    active.writer_status(&pr(NO_TOUR), AgentStatus::Working);
    active.writer_screen(&pr(NO_TOUR), vec!["Reading the outline".to_owned()]);

    let (_, writing) = active.loading().unwrap();
    assert_eq!(writing.status, Some(AgentStatus::Working));
    assert_eq!(writing.screen, ["Reading the outline"]);
    assert_eq!(writing.stage, WritingStage::Writing);
}

#[test]
fn a_writer_that_goes_idle_after_working_without_opening_failed_and_keeps_its_last_lines() {
    let fake = Arc::new(FakeTourAgent::new());
    let mut active = showing_a_pr();
    pick_no_tour(&mut active, &fake);
    active.writer_status(&pr(NO_TOUR), AgentStatus::Idle);
    assert_eq!(
        loading_stage(&active),
        WritingStage::Writing,
        "idle before its first turn is the agent starting up"
    );

    active.writer_status(&pr(NO_TOUR), AgentStatus::Working);
    active.writer_screen(&pr(NO_TOUR), vec!["Permission denied: mkdir".to_owned()]);
    active.writer_status(&pr(NO_TOUR), AgentStatus::Idle);

    let WritingStage::Failed(reason) = loading_stage(&active) else {
        panic!("failed");
    };
    assert!(reason.contains("without a tour"), "{reason}");
    assert_eq!(
        active.loading().unwrap().1.screen,
        ["Permission denied: mkdir"]
    );
    assert_eq!(
        active.tour_fact_of(&pr(NO_TOUR)),
        Some(TourFact::WritingFailed)
    );
}

#[test]
fn a_writer_whose_terminal_ended_failed() {
    let fake = Arc::new(FakeTourAgent::new());
    let mut active = showing_a_pr();
    pick_no_tour(&mut active, &fake);

    active.writer_gone(&pr(NO_TOUR));

    assert!(matches!(loading_stage(&active), WritingStage::Failed(_)));
}

#[test]
fn a_writer_that_could_not_start_failed_with_the_reason() {
    let fake = Arc::new(
        FakeTourAgent::new()
            .app_started("t1", SHOWN, AgentStatus::Working, 1)
            .app_started("t2", ELSEWHERE, AgentStatus::Working, 2)
            .app_started(
                "t3",
                "https://github.com/acme/shop/pull/11",
                AgentStatus::Working,
                3,
            ),
    );
    let mut active = showing_a_pr();
    active.write(pr(NO_TOUR));

    let started = agents(&fake).write_tour(&pr(NO_TOUR), &pr(SHOWN));
    active.writer_started(&pr(NO_TOUR), started);

    let WritingStage::Failed(reason) = loading_stage(&active) else {
        panic!("failed");
    };
    assert!(reason.contains("already running"), "{reason}");
}

#[test]
fn trying_again_starts_a_new_writer_and_picking_a_writing_pr_only_shows_it() {
    let fake = Arc::new(FakeTourAgent::new());
    let mut active = showing_a_pr();
    pick_no_tour(&mut active, &fake);
    assert_eq!(active.write(pr(NO_TOUR)), WriteDecision::Watch);
    active.writer_gone(&pr(NO_TOUR));
    assert_eq!(active.write(pr(NO_TOUR)), WriteDecision::Watch);

    active.try_again(&pr(NO_TOUR));

    assert_eq!(loading_stage(&active), WritingStage::Starting);
}

#[test]
fn the_writers_open_completes_the_loading_screen_with_the_new_tour() {
    let fake = Arc::new(FakeTourAgent::new());
    let mut active = showing_a_pr();
    pick_no_tour(&mut active, &fake);

    let OpenDecision::Build(ticket) = active.request(pr(NO_TOUR), OpenOrigin::AppStartedAgent)
    else {
        panic!("the PR the reviewer waits for is built");
    };
    let Finished::Switch(()) = active.finished(ticket, Ok(())) else {
        panic!("and shown");
    };

    assert!(active.loading().is_none());
    assert_eq!(active.shown(), Some(&pr(NO_TOUR)));
    assert_eq!(active.tour_fact_of(&pr(NO_TOUR)), None, "no longer writing");
}

#[test]
fn a_tour_finished_for_a_pr_the_reviewer_left_does_not_switch() {
    let fake = Arc::new(FakeTourAgent::new());
    let mut active = showing_a_pr();
    pick_no_tour(&mut active, &fake);
    active.stop_waiting();
    assert_eq!(active.tour_fact_of(&pr(NO_TOUR)), Some(TourFact::Writing));

    let decision = active.request(pr(NO_TOUR), OpenOrigin::AppStartedAgent);

    assert!(matches!(decision, OpenDecision::KeepInBackground));
    assert_eq!(active.shown(), Some(&pr(SHOWN)));
    assert_eq!(
        active.tour_fact_of(&pr(NO_TOUR)),
        None,
        "the disk says ready"
    );
}

#[test]
fn switching_to_another_pr_leaves_the_loading_screen_and_the_writing_goes_on() {
    let fake = Arc::new(FakeTourAgent::new());
    let mut active = showing_a_pr();
    pick_no_tour(&mut active, &fake);

    let decision = active.request(pr(ELSEWHERE), OpenOrigin::Reviewer);

    assert!(matches!(decision, OpenDecision::Build(_)));
    assert!(active.loading().is_none());
    assert_eq!(active.tour_fact_of(&pr(NO_TOUR)), Some(TourFact::Writing));
}

#[test]
fn an_open_from_an_app_started_agent_of_another_pr_does_not_switch_but_the_reviewers_does() {
    let mut active = showing_a_pr();

    let from_agent = active.request(pr(ELSEWHERE), OpenOrigin::AppStartedAgent);
    let from_reviewer = active.request(pr(ELSEWHERE), OpenOrigin::Reviewer);

    assert!(matches!(from_agent, OpenDecision::KeepInBackground));
    assert!(matches!(from_reviewer, OpenDecision::Build(_)));
}

#[test]
fn an_open_from_an_app_started_agent_of_the_shown_pr_reloads_it() {
    let mut active = showing_a_pr();

    let decision = active.request(pr(SHOWN), OpenOrigin::AppStartedAgent);

    assert!(matches!(decision, OpenDecision::Reload));
}

#[test]
fn an_agent_reply_to_a_pr_not_shown_is_unseen_until_the_pr_is_shown() {
    let mut active = showing_a_pr();
    active.agent_replied(&pr(SHOWN));
    active.agent_replied(&pr(ELSEWHERE));
    assert!(!active.has_unseen_reply(&pr(SHOWN)));
    assert!(active.has_unseen_reply(&pr(ELSEWHERE)));

    let OpenDecision::Build(ticket) = active.request(pr(ELSEWHERE), OpenOrigin::Reviewer) else {
        panic!("builds");
    };
    let Finished::Switch(()) = active.finished(ticket, Ok(())) else {
        panic!("shows");
    };

    assert!(!active.has_unseen_reply(&pr(ELSEWHERE)));
}
