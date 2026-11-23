use std::sync::Arc;

use crate::support::FixtureRepo;
use crate::support::fake_ports::{FakeGitHub, SentLog};
use crate::support::fake_tour_agent::{FakeTourAgent, TourAgentCall};
use guided_review::review_subject::ReviewSubject;
use guided_review::viewer::{
    AgentStatus, TerminalId, TourAgentPort, TourAgentState, TourAgents, WriterHandOver,
};

const PR_9: &str = "https://github.com/acme/shop/pull/9";
const SHOWN: &str = "https://github.com/acme/shop/pull/7";
const HEAD: &str = "feature/discounts";

fn pr(url: &str) -> ReviewSubject {
    url.parse().unwrap()
}

fn any_github() -> FakeGitHub {
    FakeGitHub::with_pull("base", "head", &SentLog::default())
}

/// Agents that know from GitHub which branch the PR is opened from.
fn agents(fake: &Arc<FakeTourAgent>) -> TourAgents {
    agents_knowing(fake, any_github().with_head_branch(PR_9, HEAD))
}

fn agents_knowing(fake: &Arc<FakeTourAgent>, github: FakeGitHub) -> TourAgents {
    TourAgents::new(Arc::clone(fake) as Arc<dyn TourAgentPort>)
        .finding_head_branches_with(Arc::new(github))
        .starting_in("/clones/shop".into())
}

fn clone_on(github_repo: &str, branch: &str) -> FixtureRepo {
    let repo = FixtureRepo::cloned_from(github_repo);
    let first = repo.commit("first");
    repo.branch_from(branch, &first);
    repo
}

fn reviewers_claude_in(repo: &FixtureRepo, terminal: &str, status: AgentStatus) -> FakeTourAgent {
    FakeTourAgent::new()
        .claude_in(terminal, repo.path().to_str().unwrap())
        .with_terminal(terminal, None, status)
}

fn starts(fake: &FakeTourAgent) -> usize {
    fake.calls()
        .iter()
        .filter(|call| matches!(call, TourAgentCall::Start { .. }))
        .count()
}

fn sent(fake: &FakeTourAgent) -> Vec<(TerminalId, String)> {
    fake.calls()
        .into_iter()
        .filter_map(|call| match call {
            TourAgentCall::Send { terminal, text } => Some((terminal, text)),
            _ => None,
        })
        .collect()
}

#[test]
fn the_reviewers_idle_claude_on_the_prs_head_branch_writes_the_tour_and_becomes_its_agent() {
    let repo = clone_on("acme/shop", HEAD);
    let fake = Arc::new(reviewers_claude_in(&repo, "t5", AgentStatus::Idle));

    let handed = agents(&fake).write_tour(&pr(PR_9), &pr(SHOWN)).unwrap();

    assert_eq!(handed, WriterHandOver::Delivered(TerminalId::new("t5")));
    assert_eq!(starts(&fake), 0);
    assert_eq!(fake.agents_of(PR_9), ["t5"]);
    let sent = sent(&fake);
    let [(terminal, task)] = sent.as_slice() else {
        panic!("one task typed: {sent:?}");
    };
    assert_eq!(terminal, &TerminalId::new("t5"));
    assert!(task.contains("subagent"), "{task}");
    assert!(!task.starts_with("/tour"), "{task}");
    assert!(task.contains("Skill tool"), "{task}");
    assert!(!task.contains("whole message"), "{task}");
    assert!(
        task.contains(&format!("guided-review tour-path {PR_9}")),
        "the agent reads the tour its subagent wrote: {task}"
    );
    assert!(
        task.contains(&format!("guided-review open {PR_9}")),
        "{task}"
    );
    assert!(!task.contains(['\\', '\n']), "one line: {task}");
}

#[test]
fn the_guided_tour_of_a_basic_tour_goes_to_the_reviewers_idle_claude_on_the_head_branch() {
    let repo = clone_on("acme/shop", HEAD);
    let fake = Arc::new(reviewers_claude_in(&repo, "t5", AgentStatus::Idle));

    let handed = agents(&fake)
        .hand_writer_task(&pr(PR_9), &pr(SHOWN))
        .unwrap();

    assert_eq!(handed, WriterHandOver::Delivered(TerminalId::new("t5")));
    assert_eq!(starts(&fake), 0);
    assert_eq!(fake.agents_of(PR_9), ["t5"]);
    let sent = sent(&fake);
    let [(_, task)] = sent.as_slice() else {
        panic!("one task typed: {sent:?}");
    };
    assert!(task.contains("basic tour"), "{task}");
    assert!(task.contains("subagent"), "{task}");
    assert!(!task.starts_with("/tour"), "{task}");
}

#[test]
fn a_busy_claude_on_the_head_branch_is_waited_for_not_replaced() {
    let repo = clone_on("acme/shop", HEAD);
    let fake = Arc::new(reviewers_claude_in(&repo, "t5", AgentStatus::Working));

    let handed = agents(&fake).write_tour(&pr(PR_9), &pr(SHOWN)).unwrap();

    assert_eq!(handed, WriterHandOver::Waiting(TerminalId::new("t5")));
    assert_eq!(starts(&fake), 0);
    assert_eq!(sent(&fake), []);
    assert_eq!(fake.agents_of(PR_9), ["t5"]);
}

#[test]
fn a_claude_on_another_branch_or_in_another_repo_is_left_alone_and_a_writer_is_started() {
    let other_branch = clone_on("acme/shop", "main-work");
    let other_repo = clone_on("acme/blog", HEAD);
    let fake = Arc::new(
        reviewers_claude_in(&other_branch, "t5", AgentStatus::Idle)
            .claude_in("t6", other_repo.path().to_str().unwrap())
            .with_terminal("t6", None, AgentStatus::Idle),
    );

    let handed = agents(&fake).write_tour(&pr(PR_9), &pr(SHOWN)).unwrap();

    assert!(matches!(handed, WriterHandOver::Started(_)), "{handed:?}");
    assert_eq!(sent(&fake), []);
}

#[test]
fn with_no_head_branch_known_no_claude_is_looked_for() {
    let repo = clone_on("acme/shop", HEAD);
    let fake = Arc::new(reviewers_claude_in(&repo, "t5", AgentStatus::Idle));

    let handed = agents_knowing(&fake, any_github())
        .write_tour(&pr(PR_9), &pr(SHOWN))
        .unwrap();

    assert!(matches!(handed, WriterHandOver::Started(_)), "{handed:?}");
    assert_eq!(sent(&fake), []);
}

#[test]
fn several_claudes_on_the_head_branch_are_not_guessed_between() {
    let repo = clone_on("acme/shop", HEAD);
    let subfolder = repo.path().join("src");
    std::fs::create_dir_all(&subfolder).unwrap();
    let fake = Arc::new(
        reviewers_claude_in(&repo, "t5", AgentStatus::Idle)
            .claude_in("t6", subfolder.to_str().unwrap())
            .with_terminal("t6", None, AgentStatus::Idle),
    );

    let refused = agents(&fake).write_tour(&pr(PR_9), &pr(SHOWN)).unwrap_err();

    let reason = format!("{refused:#}");
    assert!(reason.contains(HEAD), "{reason}");
    assert!(
        reason.contains(&format!("guided-review open {PR_9}")),
        "{reason}"
    );
    assert_eq!(starts(&fake), 0);
    assert_eq!(sent(&fake), []);
}

#[test]
fn every_path_finds_the_same_agent_on_the_head_branch() {
    let repo = clone_on("acme/shop", HEAD);
    let fake = Arc::new(reviewers_claude_in(&repo, "t5", AgentStatus::Working));

    let state = agents(&fake).lookup(&pr(PR_9));

    assert_eq!(
        state,
        TourAgentState::Agent {
            terminal: TerminalId::new("t5"),
            status: AgentStatus::Working,
        }
    );
    assert_eq!(fake.agents_of(PR_9), ["t5"]);
}

#[test]
fn an_agent_the_app_started_is_never_taken_for_the_reviewers() {
    let repo = clone_on("acme/shop", HEAD);
    let fake = Arc::new(
        reviewers_claude_in(&repo, "t5", AgentStatus::Idle).app_started(
            "t5",
            "https://github.com/acme/shop/pull/8",
            AgentStatus::Idle,
            100,
        ),
    );

    assert_eq!(agents(&fake).lookup(&pr(PR_9)), TourAgentState::None);
}

#[test]
fn the_reviewers_agent_that_already_has_the_prs_label_writes_the_tour() {
    let fake = Arc::new(FakeTourAgent::new().with_terminal("mine", Some(PR_9), AgentStatus::Idle));

    let handed = agents(&fake).write_tour(&pr(PR_9), &pr(SHOWN)).unwrap();

    assert_eq!(handed, WriterHandOver::Delivered(TerminalId::new("mine")));
    assert_eq!(starts(&fake), 0);
    assert_eq!(fake.agents_of(PR_9), ["mine"]);
}
