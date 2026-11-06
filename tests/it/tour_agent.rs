use crate::support::fake_tour_agent::{FakeTourAgent, TourAgentCall};
use guided_review::pull_request::PullRequestUrl;
use guided_review::viewer::{AgentStatus, TerminalId, TourAgentPort, TourAgentState, TourAgents};
use std::ops::ControlFlow;
use std::sync::Arc;

const PR: &str = "https://github.com/acme/shop/pull/7";
const OTHER_PR: &str = "https://github.com/acme/shop/pull/8";

fn pr() -> PullRequestUrl {
    PR.parse().unwrap()
}

fn agents(fake: &Arc<FakeTourAgent>) -> TourAgents {
    TourAgents::new(Arc::clone(fake) as Arc<dyn TourAgentPort>)
}

fn on(terminal: &str, status: AgentStatus) -> TourAgentState {
    TourAgentState::Agent {
        terminal: TerminalId::new(terminal),
        status,
    }
}

#[test]
fn the_agent_of_a_pr_is_the_terminal_carrying_its_label_with_its_status() {
    let fake = Arc::new(
        FakeTourAgent::new()
            .with_terminal("t1", Some(OTHER_PR), AgentStatus::Idle)
            .with_terminal("t2", Some(PR), AgentStatus::Working),
    );

    assert_eq!(agents(&fake).lookup(&pr()), on("t2", AgentStatus::Working));
}

#[test]
fn a_pr_with_no_labelled_terminal_has_no_tour_agent() {
    let fake = Arc::new(FakeTourAgent::new().with_terminal("t1", None, AgentStatus::Idle));

    assert_eq!(agents(&fake).lookup(&pr()), TourAgentState::None);
}

#[test]
fn claiming_a_terminal_makes_it_the_only_agent_of_the_pr() {
    let fake = Arc::new(
        FakeTourAgent::new()
            .with_terminal("t1", Some(PR), AgentStatus::Idle)
            .with_terminal("t2", None, AgentStatus::Idle),
    );

    agents(&fake).claim(&pr(), &TerminalId::new("t2")).unwrap();

    assert_eq!(fake.agents_of(PR), ["t2"]);
    assert_eq!(
        fake.calls(),
        [
            TourAgentCall::Release(TerminalId::new("t1")),
            TourAgentCall::Claim {
                terminal: TerminalId::new("t2"),
                pr: pr()
            }
        ]
    );
}

#[test]
fn claiming_the_terminal_that_already_is_the_agent_releases_nothing() {
    let fake = Arc::new(FakeTourAgent::new().with_terminal("t1", Some(PR), AgentStatus::Idle));

    agents(&fake).claim(&pr(), &TerminalId::new("t1")).unwrap();

    assert_eq!(fake.agents_of(PR), ["t1"]);
    assert!(
        !fake
            .calls()
            .contains(&TourAgentCall::Release(TerminalId::new("t1")))
    );
}

#[test]
fn a_terminal_reused_for_another_pr_stops_being_the_agent_of_the_first() {
    let fake = Arc::new(FakeTourAgent::new().with_terminal("t1", Some(PR), AgentStatus::Idle));
    let other: PullRequestUrl = OTHER_PR.parse().unwrap();

    agents(&fake).claim(&other, &TerminalId::new("t1")).unwrap();

    assert_eq!(agents(&fake).lookup(&pr()), TourAgentState::None);
    assert_eq!(fake.agents_of(OTHER_PR), ["t1"]);
}

#[test]
fn status_changes_are_followed_until_the_stream_ends() {
    let fake = Arc::new(
        FakeTourAgent::new()
            .with_terminal("t1", Some(PR), AgentStatus::Idle)
            .then_reports(&[AgentStatus::Working, AgentStatus::WaitingForYou]),
    );
    let mut seen = Vec::new();

    agents(&fake).follow(&pr(), &mut |state| {
        seen.push(state);
        ControlFlow::Continue(())
    });

    assert_eq!(
        seen,
        [
            on("t1", AgentStatus::Idle),
            on("t1", AgentStatus::Working),
            on("t1", AgentStatus::WaitingForYou)
        ]
    );
}

#[test]
fn an_open_request_from_a_herdr_terminal_claims_it_and_one_from_outside_claims_nothing() {
    let fake = Arc::new(FakeTourAgent::new().with_terminal("t2", None, AgentStatus::Idle));

    agents(&fake).claim_for_open(&pr(), None).unwrap();
    assert_eq!(fake.calls().len(), 0);

    agents(&fake).claim_for_open(&pr(), Some("t2")).unwrap();
    assert_eq!(fake.agents_of(PR), ["t2"]);
}

#[test]
fn following_stops_when_nobody_wants_the_changes_any_more() {
    let fake = Arc::new(
        FakeTourAgent::new()
            .with_terminal("t1", Some(PR), AgentStatus::Idle)
            .then_reports(&[AgentStatus::Working, AgentStatus::WaitingForYou]),
    );
    let mut seen = Vec::new();

    agents(&fake).follow(&pr(), &mut |state| {
        seen.push(state);
        if seen.len() == 2 {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    });

    assert_eq!(seen.len(), 2);
}

#[test]
fn without_herdr_the_agent_link_is_off_and_says_why() {
    let fake = Arc::new(FakeTourAgent::new().without_herdr("hc is not on PATH"));

    let state = agents(&fake).lookup(&pr());

    assert_eq!(
        state,
        TourAgentState::LinkOff("hc is not on PATH".to_owned())
    );
    assert!(agents(&fake).claim(&pr(), &TerminalId::new("t1")).is_err());
    assert_eq!(fake.calls().len(), 0);
}

#[test]
fn show_agent_brings_up_the_terminal_of_the_pr() {
    let fake = Arc::new(FakeTourAgent::new().with_terminal("t2", Some(PR), AgentStatus::Idle));

    agents(&fake).show(&pr()).unwrap();

    assert_eq!(fake.calls(), [TourAgentCall::Show(TerminalId::new("t2"))]);
}

#[test]
fn show_agent_with_no_agent_says_so() {
    let fake = Arc::new(FakeTourAgent::new());

    let error = agents(&fake).show(&pr()).unwrap_err();

    assert!(error.to_string().contains("no tour agent"));
}

#[test]
fn the_sidebar_line_names_the_agent_and_its_status() {
    assert_eq!(
        on("t2", AgentStatus::Idle).to_string(),
        "Tour agent t2: idle"
    );
    assert_eq!(
        on("t2", AgentStatus::Working).to_string(),
        "Tour agent t2: working"
    );
    assert_eq!(
        on("t2", AgentStatus::WaitingForYou).to_string(),
        "Tour agent t2: waiting for you"
    );
    assert_eq!(TourAgentState::None.to_string(), "Tour agent: none");
    assert!(
        TourAgentState::LinkOff("hc is not on PATH".to_owned())
            .to_string()
            .contains("hc is not on PATH")
    );
}
