use std::sync::Arc;

use crate::support::FixtureRepo;
use crate::support::fake_ports::{FakeHost, SentLog};
use crate::support::fake_tour_agent::{FakeTourAgent, TourAgentCall};
use guided_review::review_subject::{BranchName, ReviewSubject};
use guided_review::viewer::{
    AgentChoice, AgentLinkOff, AgentRole, AgentStatus, TerminalId, TourAgentPort, TourAgentState,
    TourAgents,
};

const PR_9: &str = "https://github.com/acme/shop/pull/9";
const HEAD: &str = "feature/discounts";

fn pr() -> ReviewSubject {
    PR_9.parse().unwrap()
}

fn agents(fake: &Arc<FakeTourAgent>) -> TourAgents {
    TourAgents::new(Arc::clone(fake) as Arc<dyn TourAgentPort>).finding_head_branches_with(
        Arc::new(
            FakeHost::with_pull("base", "head", &SentLog::default()).with_head_branch(PR_9, HEAD),
        ),
    )
}

fn clone_on(github_repo: &str, branch: &str) -> FixtureRepo {
    let repo = FixtureRepo::cloned_from(github_repo);
    let first = repo.commit("first");
    repo.branch_from(branch, &first);
    repo
}

fn path(repo: &FixtureRepo) -> &str {
    repo.path().to_str().unwrap()
}

#[test]
fn every_claude_terminal_is_offered_with_where_it_works_the_labelled_one_first_then_those_on_the_head_branch()
 {
    let blog = clone_on("acme/blog", "main");
    let head = clone_on("acme/shop", HEAD);
    let fake = Arc::new(
        FakeTourAgent::new()
            .claude_in("t1", path(&blog))
            .with_terminal("t1", None, AgentStatus::Working)
            .claude_in("t2", path(&head))
            .with_terminal("t2", None, AgentStatus::Idle)
            .claude_in("t3", path(&blog))
            .with_terminal("t3", Some(PR_9), AgentStatus::Idle)
            .shell_in("t4", path(&head)),
    );

    let choices = agents(&fake).agent_choices(&pr(), AgentRole::Tour).unwrap();

    let order: Vec<&str> = choices
        .iter()
        .map(|choice| choice.terminal.as_str())
        .collect();
    assert_eq!(order, ["t3", "t2", "t1"]);
    assert_eq!(
        choices[1],
        AgentChoice {
            terminal: TerminalId::new("t2"),
            space: "herdr".to_owned(),
            folder: head.path().to_path_buf(),
            repo: Some("acme/shop".parse().unwrap()),
            branch: Some(BranchName::new(HEAD).unwrap()),
            status: AgentStatus::Idle,
            is_tour_agent: false,
            is_review_agent: false,
            works_on_the_code: true,
            started_by_app: false,
        }
    );
    assert!(choices[0].is_tour_agent && !choices[0].works_on_the_code);
    assert_eq!(fake.agents_of(PR_9), ["t3"], "looking changes no label");
}

#[test]
fn for_the_review_agent_its_review_agent_comes_first_and_each_choice_says_which_agent_it_is() {
    let head = clone_on("acme/shop", HEAD);
    let fake = Arc::new(
        FakeTourAgent::new()
            .claude_in("t1", path(&head))
            .app_started("t1", PR_9, AgentStatus::Idle, 0)
            .claude_in("t2", path(&head))
            .review_agent("t2", PR_9, AgentStatus::Working, 0),
    );

    let choices = agents(&fake)
        .agent_choices(&pr(), AgentRole::Review)
        .unwrap();

    let marks: Vec<(&str, bool, bool, bool)> = choices
        .iter()
        .map(|choice| {
            (
                choice.terminal.as_str(),
                choice.is_tour_agent,
                choice.is_review_agent,
                choice.started_by_app,
            )
        })
        .collect();
    assert_eq!(
        marks,
        [("t2", false, true, true), ("t1", true, false, true)]
    );
}

#[test]
fn a_choice_matches_a_search_when_every_word_is_in_its_space_folder_repo_or_branch() {
    let choice = AgentChoice {
        terminal: TerminalId::new("t6k"),
        space: "claude-a".to_owned(),
        folder: "/Users/you/Code/claude-a".into(),
        repo: Some("acme/shop".parse().unwrap()),
        branch: Some(BranchName::new("fix/412-short-expiry").unwrap()),
        status: AgentStatus::Idle,
        is_tour_agent: false,
        is_review_agent: false,
        works_on_the_code: false,
        started_by_app: false,
    };

    assert!(choice.matches(""));
    assert!(choice.matches("CLAUDE-a"));
    assert!(choice.matches("shop expiry"));
    assert!(choice.matches("t6k"));
    assert!(!choice.matches("claude-b"));
    assert!(!choice.matches("shop dotfiles"));
}

#[test]
fn showing_a_terminal_brings_that_terminal_to_the_front() {
    let fake = Arc::new(FakeTourAgent::new().claude("t2", "herdr"));

    agents(&fake).show_terminal(&TerminalId::new("t2")).unwrap();

    assert_eq!(fake.calls(), [TourAgentCall::Show(TerminalId::new("t2"))]);
}

#[test]
fn the_agent_can_be_picked_by_hand_while_herdr_answers() {
    let agent = TourAgentState::Agent {
        terminal: TerminalId::new("t1"),
        status: AgentStatus::Idle,
    };
    let several = TourAgentState::Choose(vec![TerminalId::new("t1"), TerminalId::new("t2")]);

    assert!(agent.can_pick());
    assert!(several.can_pick());
    assert!(TourAgentState::None.can_pick());
    assert!(!TourAgentState::Unknown.can_pick());
    assert!(!TourAgentState::LinkOff(AgentLinkOff::ServerNotRunning).can_pick());
}
