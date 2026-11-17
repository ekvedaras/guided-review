use std::sync::Arc;

use crate::support::fake_tour_agent::{FakeTourAgent, TourAgentCall};
use guided_review::review_subject::ReviewSubject;
use guided_review::viewer::{AgentStatus, TerminalId, TourAgentPort, TourAgents, WriterHandOver};

const PR: &str = "https://github.com/acme/shop/pull/7";

fn pr() -> ReviewSubject {
    PR.parse().unwrap()
}

fn agents(fake: &Arc<FakeTourAgent>) -> TourAgents {
    TourAgents::new(Arc::clone(fake) as Arc<dyn TourAgentPort>).starting_in("/clones/shop".into())
}

#[test]
fn an_idle_agent_is_typed_the_writer_task_and_not_ended() {
    let fake = Arc::new(FakeTourAgent::new().with_terminal("t3", Some(PR), AgentStatus::Idle));

    let handed = agents(&fake).hand_writer_task(&pr(), &pr()).unwrap();

    assert_eq!(handed, WriterHandOver::Delivered(TerminalId::new("t3")));
    let calls = fake.calls();
    let [TourAgentCall::Send { terminal, text }] = calls.as_slice() else {
        panic!("one send and nothing else, got {calls:?}");
    };
    assert_eq!(terminal, &TerminalId::new("t3"));
    assert!(text.starts_with(&format!("/tour {PR}")));
    assert!(text.contains("basic tour"), "{text}");
    assert!(text.contains("guided-review tour-path"), "{text}");
}

#[test]
fn a_working_agent_finishes_its_hand_off_first_so_the_task_waits() {
    let fake = Arc::new(FakeTourAgent::new().with_terminal("t3", Some(PR), AgentStatus::Working));

    let handed = agents(&fake).hand_writer_task(&pr(), &pr()).unwrap();

    assert_eq!(handed, WriterHandOver::Waiting(TerminalId::new("t3")));
    assert_eq!(fake.calls(), vec![]);
}

#[test]
fn an_agent_that_waits_for_the_reviewer_is_not_typed_past() {
    let fake =
        Arc::new(FakeTourAgent::new().with_terminal("t3", Some(PR), AgentStatus::WaitingForYou));

    let handed = agents(&fake).hand_writer_task(&pr(), &pr()).unwrap();

    assert_eq!(handed, WriterHandOver::Waiting(TerminalId::new("t3")));
}

#[test]
fn an_agent_still_starting_keeps_the_task_waiting() {
    let fake = Arc::new(FakeTourAgent::new().with_terminal("t3", Some(PR), AgentStatus::Idle));
    fake.still_starting("t3");

    let handed = agents(&fake).hand_writer_task(&pr(), &pr()).unwrap();

    assert_eq!(handed, WriterHandOver::Waiting(TerminalId::new("t3")));
    assert_eq!(fake.calls(), vec![]);
}

#[test]
fn a_pr_with_no_agent_gets_one_started_with_the_writer_task() {
    let fake = Arc::new(FakeTourAgent::new());

    let handed = agents(&fake).hand_writer_task(&pr(), &pr()).unwrap();

    let WriterHandOver::Started(terminal) = handed else {
        panic!("started, got {handed:?}");
    };
    let calls = fake.calls();
    let [
        TourAgentCall::Start {
            first_task,
            subject: pr,
            ..
        },
    ] = calls.as_slice()
    else {
        panic!("one start and nothing else, got {calls:?}");
    };
    assert_eq!(pr.to_string(), PR);
    assert!(
        first_task.starts_with(&format!("/tour {PR}")),
        "{first_task}"
    );
    assert_eq!(fake.agents_of(PR), vec![terminal.to_string()]);
}

#[test]
fn with_the_agent_link_off_the_task_is_not_handed_and_says_why() {
    let fake = Arc::new(
        FakeTourAgent::new().without_herdr(guided_review::viewer::AgentLinkOff::ServerNotRunning),
    );

    let error = agents(&fake).hand_writer_task(&pr(), &pr()).unwrap_err();

    assert!(format!("{error:#}").contains("herdr server is not running"));
}
