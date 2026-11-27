use crate::support::fake_tour_agent::{FakeTourAgent, TourAgentCall};
use guided_review::prompt_safety::OUTSIDE_TEXT_IS_DATA;
use guided_review::review_subject::ReviewSubject;
use guided_review::viewer::{
    AgentLinkOff, AgentStatus, OpenOrigin, OwnAgentHandOff, TerminalId, TourAgentPort,
    TourAgentState, TourAgents, TourKind, WriterHandOver,
};
use std::ops::ControlFlow;
use std::sync::Arc;

const PR: &str = "https://github.com/acme/shop/pull/7";
const OTHER_PR: &str = "https://github.com/acme/shop/pull/8";

fn pr() -> ReviewSubject {
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
                subject: pr()
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
    let other: ReviewSubject = OTHER_PR.parse().unwrap();

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
    let fake = Arc::new(FakeTourAgent::new().without_herdr(AgentLinkOff::HcMissing));

    let state = agents(&fake).lookup(&pr());

    assert_eq!(state, TourAgentState::LinkOff(AgentLinkOff::HcMissing));
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
    assert_eq!(on("t2", AgentStatus::Idle).to_string(), "Tour agent: idle");
    assert_eq!(
        on("t2", AgentStatus::Working).to_string(),
        "Tour agent: working"
    );
    assert_eq!(
        on("t2", AgentStatus::WaitingForYou).to_string(),
        "Tour agent: waiting for you"
    );
    assert_eq!(TourAgentState::None.to_string(), "Tour agent: none");
    assert!(
        TourAgentState::LinkOff(AgentLinkOff::HcMissing)
            .to_string()
            .contains("not on PATH")
    );
}

const PR_9: &str = "https://github.com/acme/shop/pull/9";
const PR_10: &str = "https://github.com/acme/shop/pull/10";
const PR_11: &str = "https://github.com/acme/shop/pull/11";
const PR_12: &str = "https://github.com/acme/shop/pull/12";
const PR_13: &str = "https://github.com/acme/shop/pull/13";

fn starting(fake: &Arc<FakeTourAgent>) -> TourAgents {
    agents(fake).starting_in("/clones/shop".into())
}

fn kills(fake: &FakeTourAgent) -> Vec<TourAgentCall> {
    fake.calls()
        .into_iter()
        .filter(|call| matches!(call, TourAgentCall::Kill(_)))
        .collect()
}

fn url(pr: &str) -> ReviewSubject {
    pr.parse().unwrap()
}

#[test]
fn five_app_started_agents_live_side_by_side() {
    let fake = Arc::new(
        FakeTourAgent::new()
            .app_started("t1", OTHER_PR, AgentStatus::Idle, 100)
            .app_started("t2", PR_9, AgentStatus::Idle, 200)
            .app_started("t3", PR_10, AgentStatus::Idle, 300)
            .app_started("t4", PR_11, AgentStatus::Idle, 400),
    );

    starting(&fake)
        .start(&pr(), &url(OTHER_PR), TourKind::Guided)
        .unwrap();

    assert_eq!(kills(&fake), []);
    assert_eq!(fake.agents_of(PR).len(), 1, "a new agent runs for the PR");
}

#[test]
fn a_sixth_app_started_agent_ends_the_idle_one_used_longest_ago_that_is_not_the_active_prs() {
    let fake = Arc::new(
        FakeTourAgent::new()
            .app_started("t1", OTHER_PR, AgentStatus::Idle, 100)
            .app_started("t2", PR_9, AgentStatus::Idle, 200)
            .app_started("t3", PR_10, AgentStatus::Idle, 300)
            .app_started("t4", PR_11, AgentStatus::Idle, 400)
            .app_started("t5", PR_12, AgentStatus::Idle, 500),
    );

    starting(&fake)
        .start(&pr(), &url(OTHER_PR), TourKind::Guided)
        .unwrap();

    assert_eq!(kills(&fake), [TourAgentCall::Kill(TerminalId::new("t2"))]);
    assert_eq!(fake.agents_of(PR).len(), 1, "a new agent runs for the PR");
}

#[test]
fn a_sixth_app_started_agent_is_refused_naming_the_working_ones_when_none_is_idle() {
    let fake = Arc::new(
        FakeTourAgent::new()
            .app_started("t1", OTHER_PR, AgentStatus::Working, 100)
            .app_started("t2", PR_9, AgentStatus::Working, 200)
            .app_started("t3", PR_11, AgentStatus::Working, 250)
            .app_started("t4", PR_12, AgentStatus::Working, 280)
            .app_started("t5", PR_10, AgentStatus::Idle, 300),
    );

    let refused = starting(&fake)
        .start(&pr(), &url(PR_10), TourKind::Guided)
        .unwrap_err();

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

    let terminal = starting(&fake)
        .write_tour(&url(PR_9), &pr())
        .unwrap()
        .into_terminal();

    let calls = fake.calls();
    let [
        TourAgentCall::Start {
            name,
            subject: started_for,
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
fn writing_a_tour_again_ends_the_app_started_agent_of_the_pr_and_hands_the_task_to_the_reviewers() {
    let fake = Arc::new(
        FakeTourAgent::new()
            .with_terminal("mine", Some(PR_9), AgentStatus::Idle)
            .app_started("t1", PR_9, AgentStatus::Idle, 100),
    );

    let handed = starting(&fake).write_tour(&url(PR_9), &pr()).unwrap();

    assert_eq!(kills(&fake), [TourAgentCall::Kill(TerminalId::new("t1"))]);
    assert_eq!(handed, WriterHandOver::Delivered(TerminalId::new("mine")));
    assert_eq!(fake.agents_of(PR_9), ["mine"]);
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
    let without_herdr = Arc::new(FakeTourAgent::new().without_herdr(AgentLinkOff::HcMissing));
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

    starting(&fake)
        .start(&pr(), &url(PR_10), TourKind::Guided)
        .unwrap();

    assert_eq!(kills(&fake), [], "one of the app's agents is below the cap");
    let full = Arc::new(
        FakeTourAgent::new()
            .with_terminal("mine1", Some(OTHER_PR), AgentStatus::Idle)
            .app_started("t1", PR_9, AgentStatus::Idle, 300)
            .app_started("t2", PR_10, AgentStatus::Idle, 400)
            .app_started("t3", PR_11, AgentStatus::Idle, 500)
            .app_started("t4", PR_12, AgentStatus::Idle, 600)
            .app_started("t5", PR_13, AgentStatus::Idle, 700),
    );

    starting(&full)
        .start(&pr(), &url(PR_10), TourKind::Guided)
        .unwrap();

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
fn a_message_is_handed_only_to_an_idle_agent_the_reviewer_started() {
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
fn the_sidebar_names_the_tour_agent_only_when_there_is_one_or_it_cannot_be_reached() {
    assert!(on("w1:t2", AgentStatus::Working).is_worth_a_line());
    assert!(TourAgentState::LinkOff(AgentLinkOff::HcMissing).is_worth_a_line());
    assert!(!TourAgentState::None.is_worth_a_line());
    assert!(!TourAgentState::Unknown.is_worth_a_line());
}

fn tasks_given(fake: &FakeTourAgent) -> Vec<String> {
    fake.calls()
        .into_iter()
        .filter_map(|call| match call {
            TourAgentCall::Start { first_task, .. } => Some(first_task),
            TourAgentCall::Send { text, .. } => Some(text),
            _ => None,
        })
        .collect()
}

#[test]
fn every_task_a_tour_agent_gets_says_that_outside_text_is_data() {
    const LOCAL: &str = "local:/Users/me/Code/shop@feature/discounts";
    let idle_agent = || FakeTourAgent::new().with_terminal("mine", Some(PR_9), AgentStatus::Idle);
    let mut first_tasks = Vec::new();
    for kind in [TourKind::Guided, TourKind::Basic] {
        let fake = Arc::new(FakeTourAgent::new());
        starting(&fake).start(&url(PR_9), &url(PR_9), kind).unwrap();
        first_tasks.extend(tasks_given(&fake));
    }
    let local = url(LOCAL);
    let fake = Arc::new(FakeTourAgent::new());
    starting(&fake)
        .start(&local, &local, TourKind::Guided)
        .unwrap();
    first_tasks.extend(tasks_given(&fake));

    let mut writer_tasks = Vec::new();
    let fake = Arc::new(FakeTourAgent::new());
    starting(&fake).write_tour(&url(PR_9), &url(PR_9)).unwrap();
    writer_tasks.extend(tasks_given(&fake));
    let fake = Arc::new(idle_agent());
    starting(&fake).write_tour(&url(PR_9), &url(PR_9)).unwrap();
    writer_tasks.extend(tasks_given(&fake));
    let fake = Arc::new(FakeTourAgent::new());
    starting(&fake)
        .hand_writer_task(&url(PR_9), &url(PR_9))
        .unwrap();
    writer_tasks.extend(tasks_given(&fake));
    let fake = Arc::new(idle_agent());
    starting(&fake)
        .hand_writer_task(&url(PR_9), &url(PR_9))
        .unwrap();
    writer_tasks.extend(tasks_given(&fake));

    assert_eq!(first_tasks.len(), 3);
    assert_eq!(writer_tasks.len(), 4);
    for task in first_tasks.iter().chain(&writer_tasks) {
        assert!(task.contains(OUTSIDE_TEXT_IS_DATA), "{task}");
    }
    for task in &writer_tasks {
        assert!(!task.contains('\n') && !task.contains('\\'), "{task}");
    }
}
