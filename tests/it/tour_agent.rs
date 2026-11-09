use crate::support::fake_tour_agent::{FakeTourAgent, TourAgentCall};
use guided_review::pull_request::PullRequestUrl;
use guided_review::viewer::{
    AgentStatus, OpenOrigin, OwnAgentHandOff, TerminalId, TourAgentPort, TourAgentState, TourAgents,
};
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
fn an_open_request_from_outside_herdr_claims_nothing() {
    let fake = Arc::new(FakeTourAgent::new().claude("t2", "shop"));

    agents(&fake).claim_for_open(&pr(), None).unwrap();

    assert_eq!(fake.calls().len(), 0);
}

#[test]
fn an_open_request_from_a_terminal_running_claude_claims_it() {
    let fake = Arc::new(
        FakeTourAgent::new()
            .claude("t2", "shop")
            .claude("t3", "shop"),
    );

    agents(&fake).claim_for_open(&pr(), Some("t2")).unwrap();

    assert_eq!(fake.agents_of(PR), ["t2"]);
}

#[test]
fn an_open_request_from_a_plain_shell_claims_the_one_claude_terminal_of_its_space() {
    let fake = Arc::new(
        FakeTourAgent::new()
            .shell("t1", "shop")
            .claude("t2", "shop")
            .claude("t3", "elsewhere")
            .shell("t4", "shop"),
    );

    agents(&fake).claim_for_open(&pr(), Some("t1")).unwrap();

    assert_eq!(fake.agents_of(PR), ["t2"]);
}

#[test]
fn an_open_request_from_a_plain_shell_claims_nothing_when_its_space_has_several_claude_terminals() {
    let fake = Arc::new(
        FakeTourAgent::new()
            .shell("t1", "shop")
            .claude("t2", "shop")
            .claude("t3", "shop"),
    );

    agents(&fake).claim_for_open(&pr(), Some("t1")).unwrap();

    assert_eq!(fake.calls().len(), 0);
}

#[test]
fn an_open_request_from_a_plain_shell_claims_nothing_when_its_space_has_no_claude_terminal() {
    let fake = Arc::new(
        FakeTourAgent::new()
            .shell("t1", "shop")
            .shell("t2", "shop")
            .claude("t3", "elsewhere"),
    );

    agents(&fake).claim_for_open(&pr(), Some("t1")).unwrap();

    assert_eq!(fake.calls().len(), 0);
}

#[test]
fn an_open_request_from_a_terminal_herdr_does_not_list_claims_nothing() {
    let fake = Arc::new(FakeTourAgent::new().claude("t2", "shop"));

    agents(&fake).claim_for_open(&pr(), Some("t9")).unwrap();

    assert_eq!(fake.calls().len(), 0);
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

const PR_9: &str = "https://github.com/acme/shop/pull/9";
const PR_10: &str = "https://github.com/acme/shop/pull/10";
const PR_11: &str = "https://github.com/acme/shop/pull/11";

fn starting(fake: &Arc<FakeTourAgent>) -> TourAgents {
    agents(fake).starting_in("/clones/shop".into())
}

fn kills(fake: &FakeTourAgent) -> Vec<TourAgentCall> {
    fake.calls()
        .into_iter()
        .filter(|call| matches!(call, TourAgentCall::Kill(_)))
        .collect()
}

fn url(pr: &str) -> PullRequestUrl {
    pr.parse().unwrap()
}

#[test]
fn a_fourth_app_started_agent_ends_the_idle_one_used_longest_ago_that_is_not_the_active_prs() {
    let fake = Arc::new(
        FakeTourAgent::new()
            .app_started("t1", OTHER_PR, AgentStatus::Idle, 100)
            .app_started("t2", PR_9, AgentStatus::Idle, 200)
            .app_started("t3", PR_10, AgentStatus::Idle, 300),
    );

    starting(&fake).start(&pr(), &url(OTHER_PR)).unwrap();

    assert_eq!(kills(&fake), [TourAgentCall::Kill(TerminalId::new("t2"))]);
    assert_eq!(fake.agents_of(PR).len(), 1, "a new agent runs for the PR");
}

#[test]
fn a_fourth_app_started_agent_is_refused_naming_the_working_ones_when_none_is_idle() {
    let fake = Arc::new(
        FakeTourAgent::new()
            .app_started("t1", OTHER_PR, AgentStatus::Working, 100)
            .app_started("t2", PR_9, AgentStatus::Working, 200)
            .app_started("t3", PR_10, AgentStatus::Idle, 300),
    );

    let refused = starting(&fake).start(&pr(), &url(PR_10)).unwrap_err();

    let reason = format!("{refused:#}");
    assert!(
        reason.contains(OTHER_PR) && reason.contains(PR_9),
        "{reason}"
    );
    assert!(!reason.contains(PR_10), "{reason}");
    assert_eq!(kills(&fake), []);
    assert_eq!(fake.agents_of(PR), Vec::<String>::new());
}

#[test]
fn writing_a_tour_starts_the_prs_one_tour_agent_in_its_folder_on_the_tour_skill() {
    let fake = Arc::new(FakeTourAgent::new());

    let terminal = starting(&fake).write_tour(&url(PR_9), &pr()).unwrap();

    let calls = fake.calls();
    let [
        TourAgentCall::Start {
            name,
            pr: started_for,
            folder,
            first_task,
        },
    ] = calls.as_slice()
    else {
        panic!("one start: {calls:?}");
    };
    assert_eq!(name, "tour-acme-shop-9");
    assert_eq!(started_for, &url(PR_9));
    assert_eq!(folder, std::path::Path::new("/clones/shop"));
    assert!(
        first_task.starts_with(&format!("/tour {PR_9} ")),
        "{first_task}"
    );
    assert!(
        first_task.contains("one command per Bash call"),
        "{first_task}"
    );
    assert_eq!(fake.agents_of(PR_9), [terminal.to_string()]);
}

#[test]
fn writing_a_tour_again_ends_the_app_started_agent_of_the_pr_first_and_unlabels_the_reviewers() {
    let fake = Arc::new(
        FakeTourAgent::new()
            .with_terminal("mine", Some(PR_9), AgentStatus::Idle)
            .app_started("t1", PR_9, AgentStatus::Idle, 100),
    );

    let terminal = starting(&fake).write_tour(&url(PR_9), &pr()).unwrap();

    assert_eq!(kills(&fake), [TourAgentCall::Kill(TerminalId::new("t1"))]);
    assert_eq!(fake.agents_of(PR_9), [terminal.to_string()]);
}

#[test]
fn an_open_from_a_terminal_the_app_started_comes_from_an_app_started_agent() {
    let fake = Arc::new(
        FakeTourAgent::new()
            .app_started("t1", PR_9, AgentStatus::Working, 100)
            .with_terminal("mine", Some(PR), AgentStatus::Working),
    );
    let agents = agents(&fake);

    assert_eq!(agents.origin_of(Some("t1")), OpenOrigin::AppStartedAgent);
    assert_eq!(agents.origin_of(Some("mine")), OpenOrigin::Reviewer);
    assert_eq!(agents.origin_of(None), OpenOrigin::Reviewer);
    let without_herdr = Arc::new(FakeTourAgent::new().without_herdr("no hc"));
    assert_eq!(
        self::agents(&without_herdr).origin_of(Some("t1")),
        OpenOrigin::Reviewer
    );
}

#[test]
fn agents_the_reviewer_started_are_neither_counted_nor_ended() {
    let fake = Arc::new(
        FakeTourAgent::new()
            .with_terminal("mine1", Some(OTHER_PR), AgentStatus::Idle)
            .with_terminal("mine2", Some(PR_9), AgentStatus::Idle)
            .app_started("t1", PR_10, AgentStatus::Idle, 300),
    );

    starting(&fake).start(&pr(), &url(PR_10)).unwrap();

    assert_eq!(kills(&fake), [], "one of the app's agents is below the cap");
    let full = Arc::new(
        FakeTourAgent::new()
            .with_terminal("mine1", Some(OTHER_PR), AgentStatus::Idle)
            .app_started("t1", PR_9, AgentStatus::Idle, 300)
            .app_started("t2", PR_10, AgentStatus::Idle, 400)
            .app_started("t3", PR_11, AgentStatus::Idle, 500),
    );

    starting(&full).start(&pr(), &url(PR_10)).unwrap();

    assert_eq!(kills(&full), [TourAgentCall::Kill(TerminalId::new("t1"))]);
    assert!(full.live_terminal_addresses().contains(&"mine1".to_owned()));
}

fn sends(fake: &FakeTourAgent) -> Vec<TourAgentCall> {
    fake.calls()
        .into_iter()
        .filter(|call| matches!(call, TourAgentCall::Send { .. }))
        .collect()
}

#[test]
fn an_ask_is_handed_only_to_an_idle_agent_the_reviewer_started() {
    let mine = Arc::new(FakeTourAgent::new().with_terminal("mine", Some(PR), AgentStatus::Idle));
    assert_eq!(
        agents(&mine).hand_to_own_agent(&pr(), "do it"),
        OwnAgentHandOff::Delivered(TerminalId::new("mine"))
    );
    assert_eq!(
        sends(&mine),
        [TourAgentCall::Send {
            terminal: TerminalId::new("mine"),
            text: "do it".to_owned()
        }]
    );

    let by_app = Arc::new(FakeTourAgent::new().app_started("t1", PR, AgentStatus::Idle, 100));
    assert_eq!(
        agents(&by_app).hand_to_own_agent(&pr(), "do it"),
        OwnAgentHandOff::StartedByApp(TerminalId::new("t1"))
    );
    assert_eq!(sends(&by_app), []);

    let busy = Arc::new(FakeTourAgent::new().with_terminal("mine", Some(PR), AgentStatus::Working));
    assert_eq!(
        agents(&busy).hand_to_own_agent(&pr(), "do it"),
        OwnAgentHandOff::NotSent("mine is working".to_owned())
    );
    assert_eq!(sends(&busy), []);

    let none = Arc::new(FakeTourAgent::new());
    assert_eq!(
        agents(&none).hand_to_own_agent(&pr(), "do it"),
        OwnAgentHandOff::NoAgent
    );
}

#[test]
fn the_ask_panel_line_says_what_became_of_the_hand_off() {
    assert_eq!(
        OwnAgentHandOff::Delivered(TerminalId::new("t2")).line(),
        "Handed to your agent t2"
    );
    assert_eq!(
        OwnAgentHandOff::StartedByApp(TerminalId::new("t1")).line(),
        "Not handed: the tour agent t1 was started by the app and cannot change files. Run `guided-review open` from your own Claude in herdr to make it the tour agent"
    );
    assert_eq!(
        OwnAgentHandOff::NotSent("t2 is working".to_owned()).line(),
        "Not handed: t2 is working. Try again when it is free"
    );
    assert_eq!(
        OwnAgentHandOff::NoAgent.line(),
        "Not handed: this PR has no tour agent. Run `guided-review open` from your own Claude in herdr to make it the tour agent"
    );
}
