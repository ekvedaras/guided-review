use crate::support::fake_tour_agent::{FakeTourAgent, TourAgentCall};
use guided_review::review_subject::ReviewSubject;
use guided_review::viewer::{
    AgentStatus, OwnAgentHandOff, SendOutcome, TerminalId, TourAgentPort, TourAgentState,
    TourAgents, TourKind,
};
use std::sync::Arc;

const LOCAL: &str = "local:/Users/me/Code/shop@feature/discounts";
const PR: &str = "https://github.com/acme/shop/pull/7";

fn local() -> ReviewSubject {
    LOCAL.parse().unwrap()
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

fn sends(fake: &FakeTourAgent) -> Vec<TourAgentCall> {
    fake.calls()
        .into_iter()
        .filter(|call| matches!(call, TourAgentCall::Send { .. }))
        .collect()
}

fn two_in_the_folder() -> Arc<FakeTourAgent> {
    Arc::new(
        FakeTourAgent::new()
            .claude_in("t2", "/Users/me/Code/shop")
            .claude_in("t3", "/Users/me/Code/shop"),
    )
}

#[test]
fn the_terminal_that_ran_open_is_the_agent_when_it_runs_claude() {
    let fake = Arc::new(
        FakeTourAgent::new()
            .claude_in("t1", "/Users/me/Code/shop")
            .claude_in("t2", "/Users/me/Code/shop")
            .claude_in("t3", "/Users/me/Code/elsewhere"),
    );

    agents(&fake).claim_for_open(&local(), Some("t3")).unwrap();

    assert_eq!(fake.agents_of(LOCAL), ["t3"]);
}

#[test]
fn opened_from_a_shell_the_only_claude_terminal_in_the_folder_or_below_it_is_the_agent() {
    let fake = Arc::new(
        FakeTourAgent::new()
            .shell_in("t1", "/Users/me/Code/shop")
            .claude_in("t2", "/Users/me/Code/shop/src/viewer")
            .claude_in("t3", "/Users/me/Code/shop-2")
            .claude_in("t4", "/Users/me/Code"),
    );

    agents(&fake).claim_for_open(&local(), Some("t1")).unwrap();

    assert_eq!(fake.agents_of(LOCAL), ["t2"]);
}

#[test]
fn opened_from_outside_herdr_the_only_claude_terminal_in_the_folder_is_still_found() {
    let fake = Arc::new(FakeTourAgent::new().claude_in("t2", "/Users/me/Code/shop"));

    agents(&fake).claim_for_open(&local(), None).unwrap();

    assert_eq!(fake.agents_of(LOCAL), ["t2"]);
}

#[test]
fn with_several_claude_terminals_in_the_folder_none_is_claimed() {
    let fake = Arc::new(
        FakeTourAgent::new()
            .shell_in("t1", "/Users/me/Code/shop")
            .claude_in("t2", "/Users/me/Code/shop")
            .claude_in("t3", "/Users/me/Code/shop"),
    );

    agents(&fake).claim_for_open(&local(), Some("t1")).unwrap();

    assert_eq!(fake.calls().len(), 0);
}

#[test]
fn several_claude_terminals_in_the_folder_are_offered_to_pick_from() {
    let fake = Arc::new(
        FakeTourAgent::new()
            .claude_in("t2", "/Users/me/Code/shop")
            .claude_in("t3", "/Users/me/Code/shop")
            .claude_in("t4", "/Users/me/Code/other"),
    );

    let state = agents(&fake).lookup(&local());

    assert_eq!(
        state,
        TourAgentState::Choose(vec![TerminalId::new("t2"), TerminalId::new("t3")])
    );
    assert_eq!(
        state.choices(),
        [TerminalId::new("t2"), TerminalId::new("t3")]
    );
    assert!(state.is_worth_a_line());
    assert_eq!(
        state.line_for(&local()),
        "Several agents work in this folder: `t2`, `t3`"
    );
    assert_eq!(fake.calls().len(), 0);
}

#[test]
fn picking_one_of_the_offered_agents_makes_it_the_agent() {
    let fake = two_in_the_folder();

    agents(&fake)
        .claim(&local(), &TerminalId::new("t3"))
        .unwrap();

    assert_eq!(fake.agents_of(LOCAL), ["t3"]);
    assert_eq!(
        agents(&fake).lookup(&local()),
        on("t3", AgentStatus::Unreported)
    );
}

#[test]
fn without_an_open_request_the_only_claude_terminal_in_the_folder_is_found_as_the_agent() {
    let fake = Arc::new(FakeTourAgent::new().claude_in("t2", "/Users/me/Code/shop"));

    assert_eq!(
        agents(&fake).lookup(&local()),
        on("t2", AgentStatus::Unreported)
    );
    assert_eq!(fake.agents_of(LOCAL), ["t2"]);
}

#[test]
fn with_no_claude_terminal_in_the_folder_there_is_no_agent() {
    let fake = Arc::new(
        FakeTourAgent::new()
            .shell_in("t1", "/Users/me/Code/shop")
            .claude_in("t2", "/Users/me/Code/other"),
    );

    assert_eq!(agents(&fake).lookup(&local()), TourAgentState::None);
}

#[test]
fn a_pr_never_looks_in_a_folder_for_its_agent() {
    let fake = Arc::new(FakeTourAgent::new().claude_in("t2", "/clones/shop"));

    assert_eq!(
        agents(&fake)
            .starting_in("/clones/shop".into())
            .lookup(&PR.parse().unwrap()),
        TourAgentState::None
    );
}

#[test]
fn the_sidebar_calls_a_local_reviews_agent_an_agent_and_a_prs_a_tour_agent() {
    let agent = on("t2", AgentStatus::Idle);

    assert_eq!(agent.line_for(&local()), "Agent `t2`: idle");
    assert_eq!(
        agent.line_for(&PR.parse().unwrap()),
        "Tour agent `t2`: idle"
    );
    assert_eq!(TourAgentState::None.line_for(&local()), "Agent: none");
}

#[test]
fn with_no_agent_in_the_folder_one_is_started_there_and_not_told_to_push() {
    let fake = Arc::new(FakeTourAgent::new());

    agents(&fake)
        .start(&local(), &local(), TourKind::Basic)
        .unwrap();

    let calls = fake.calls();
    let [
        TourAgentCall::Start {
            name,
            subject,
            folder,
            first_task,
        },
    ] = calls.as_slice()
    else {
        panic!("one start: {calls:?}");
    };
    assert_eq!(subject, &local());
    assert_eq!(folder, std::path::Path::new("/Users/me/Code/shop"));
    assert!(name.starts_with("local-shop-feature"), "{name}");
    assert!(name.len() <= 32, "{name}");
    assert!(
        first_task.contains("guided-review outline local:/Users/me/Code/shop@feature/discounts"),
        "{first_task}"
    );
    assert!(first_task.contains("do not push"), "{first_task}");
    assert!(!first_task.contains("PR"), "{first_task}");
    assert_eq!(fake.agents_of(LOCAL).len(), 1);
}

#[test]
fn two_change_sets_with_long_names_get_two_agent_names() {
    let started = |key: &str| {
        let subject: ReviewSubject = key.parse().unwrap();
        let fake = Arc::new(FakeTourAgent::new());
        agents(&fake)
            .start(&subject, &subject, TourKind::Basic)
            .unwrap();
        let calls = fake.calls();
        let [TourAgentCall::Start { name, .. }] = calls.as_slice() else {
            panic!("one start");
        };
        name.clone()
    };
    let one = started("local:/Users/me/Code/a-very-long-folder-name-indeed@feature/one");
    let other = started("local:/Users/me/Code/a-very-long-folder-name-indeed@feature/two");

    assert_ne!(one, other);
    assert!(one.len() <= 32 && other.len() <= 32, "{one} {other}");
}

#[test]
fn a_task_waits_while_several_agents_are_there_to_pick_from() {
    let fake = two_in_the_folder();

    let outcome = agents(&fake).send_if_idle(&local(), "a task").unwrap();

    assert!(matches!(outcome, SendOutcome::Refused(_)), "{outcome:?}");
    assert_eq!(sends(&fake), []);
}

#[test]
fn an_ask_is_handed_to_the_change_sets_agent_even_when_the_app_started_it() {
    let by_app = Arc::new(FakeTourAgent::new().app_started("t1", LOCAL, AgentStatus::Idle, 100));
    assert_eq!(
        agents(&by_app).hand_to_own_agent(&local(), "do it"),
        OwnAgentHandOff::Delivered(TerminalId::new("t1"))
    );

    let in_folder = Arc::new(FakeTourAgent::new().claude_in("t2", "/Users/me/Code/shop"));
    in_folder.set_status("t2", AgentStatus::Idle);
    assert_eq!(
        agents(&in_folder).hand_to_own_agent(&local(), "do it"),
        OwnAgentHandOff::Delivered(TerminalId::new("t2"))
    );
    assert_eq!(sends(&in_folder).len(), 1);

    assert_eq!(
        agents(&two_in_the_folder()).hand_to_own_agent(&local(), "do it"),
        OwnAgentHandOff::PickAnAgent
    );
}

#[test]
fn an_ask_for_an_agent_that_does_not_report_its_state_is_typed_in_only_when_sent_anyway() {
    let fake =
        Arc::new(FakeTourAgent::new().with_terminal("t1", Some(LOCAL), AgentStatus::Unreported));

    let held = agents(&fake).hand_to_own_agent(&local(), "do it");

    assert_eq!(held, OwnAgentHandOff::Unreported(TerminalId::new("t1")));
    assert_eq!(sends(&fake), []);
    assert!(held.can_send_anyway());

    let sent = agents(&fake).hand_to_own_agent_anyway(&local(), "do it");

    assert_eq!(sent, OwnAgentHandOff::Delivered(TerminalId::new("t1")));
    assert!(!sent.can_send_anyway());
    assert_eq!(
        sends(&fake),
        [TourAgentCall::Send {
            terminal: TerminalId::new("t1"),
            text: "do it".to_owned()
        }]
    );
}

#[test]
fn an_ask_with_no_agent_in_the_folder_starts_one_with_the_ask_as_its_task() {
    let fake = Arc::new(FakeTourAgent::new());

    let outcome = agents(&fake).hand_to_own_agent(&local(), "rename the discount");

    assert!(
        matches!(outcome, OwnAgentHandOff::Started(_)),
        "{outcome:?}"
    );
    let calls = fake.calls();
    let [
        TourAgentCall::Start {
            folder, first_task, ..
        },
    ] = calls.as_slice()
    else {
        panic!("one start: {calls:?}");
    };
    assert_eq!(folder, std::path::Path::new("/Users/me/Code/shop"));
    assert!(first_task.contains("rename the discount"), "{first_task}");
    assert!(first_task.contains("do not push"), "{first_task}");
}

#[test]
fn a_claude_in_a_nested_worktree_of_another_branch_is_not_the_folders_agent() {
    let temp = tempfile::tempdir().unwrap();
    let folder = temp.path().canonicalize().unwrap();
    let nested = folder.join(".claude/worktrees/agent-1");
    std::fs::create_dir_all(nested.join("src")).unwrap();
    std::fs::write(nested.join(".git"), "gitdir: /elsewhere").unwrap();
    let subject: ReviewSubject = format!("local:{}@feature/discounts", folder.display())
        .parse()
        .unwrap();
    let fake = Arc::new(
        FakeTourAgent::new()
            .claude_in("t2", nested.join("src").to_str().unwrap())
            .claude_in("t3", folder.to_str().unwrap()),
    );

    agents(&fake).claim_for_open(&subject, None).unwrap();

    assert_eq!(fake.agents_of(&subject.to_string()), ["t3"]);
}

#[test]
fn the_ask_panel_line_for_a_local_review_says_what_to_do_about_the_agent() {
    assert_eq!(
        OwnAgentHandOff::Started(TerminalId::new("t9")).line(),
        "Started an agent t9 in this review's folder with your Ask"
    );
    assert!(
        OwnAgentHandOff::PickAnAgent
            .line()
            .contains("Use this agent")
    );
}
