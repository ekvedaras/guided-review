//! Refresh tour is handed to the subject's tour agent, and lasts until the agent's turn ends.

use std::sync::Arc;

use crate::support::fake_tour_agent::{FakeTourAgent, TourAgentCall};
use guided_review::review_subject::ReviewSubject;
use guided_review::viewer::{
    AgentStatus, Cancel, Pace, RefreshCancelled, RefreshWriter, TourAgentPort, TourAgentRefresh,
    TourAgents,
};

const PR: &str = "https://github.com/acme/shop/pull/7";
const LOCAL: &str = "local:/work/shop@feature";

fn refresh(fake: &Arc<FakeTourAgent>, subject: &str) -> (anyhow::Result<()>, Vec<String>) {
    let agents = TourAgents::new(Arc::clone(fake) as Arc<dyn TourAgentPort>)
        .starting_in("/clones/shop".into());
    let subject: ReviewSubject = subject.parse().unwrap();
    let mut activities = Vec::new();
    let refreshed = TourAgentRefresh::new(agents, Pace::at_once()).refresh(
        &subject,
        &Cancel::default(),
        &mut |activity| {
            activities.push(activity);
        },
    );
    (refreshed, activities)
}

fn sent_text(fake: &FakeTourAgent) -> String {
    let sent: Vec<String> = fake
        .calls()
        .into_iter()
        .filter_map(|call| match call {
            TourAgentCall::Send { text, .. } => Some(text),
            _ => None,
        })
        .collect();
    let [text] = sent.as_slice() else {
        panic!("one send, got {sent:?}");
    };
    text.clone()
}

#[test]
fn an_idle_agent_is_typed_the_refresh_task_and_the_refresh_ends_when_its_turn_does() {
    let fake = Arc::new(
        FakeTourAgent::new()
            .with_terminal("t3", Some(PR), AgentStatus::Idle)
            .after_a_send_looks_report(&[
                AgentStatus::Working,
                AgentStatus::Working,
                AgentStatus::Idle,
            ]),
    );

    let (refreshed, activities) = refresh(&fake, PR);

    refreshed.unwrap();
    let text = sent_text(&fake);
    assert!(text.contains("Refresh"), "{text}");
    assert!(text.contains("subagent"), "{text}");
    assert!(
        text.contains(&format!("guided-review refresh {PR} --done")),
        "{text}"
    );
    assert!(!text.starts_with("/tour"), "{text}");
    assert_eq!(
        activities.last().unwrap(),
        "The tour agent t3 is refreshing the tour"
    );
}

#[test]
fn a_busy_agent_is_not_typed_into_and_the_refresh_fails_saying_it_was_not_free() {
    let fake = Arc::new(FakeTourAgent::new().with_terminal("t3", Some(PR), AgentStatus::Working));

    let (refreshed, _) = refresh(&fake, PR);

    let error = format!("{:#}", refreshed.unwrap_err());
    assert!(
        error.contains("t3") && error.contains("not free"),
        "{error}"
    );
    assert_eq!(fake.calls(), vec![]);
}

#[test]
fn an_agent_that_never_starts_on_the_task_fails_the_refresh() {
    let fake = Arc::new(
        FakeTourAgent::new()
            .with_terminal("t3", Some(PR), AgentStatus::Idle)
            .after_a_send_looks_report(&[AgentStatus::Idle]),
    );

    let (refreshed, _) = refresh(&fake, PR);

    let error = format!("{:#}", refreshed.unwrap_err());
    assert!(error.contains("did not start"), "{error}");
}

#[test]
fn an_agent_that_fails_while_it_works_fails_the_refresh() {
    let fake = Arc::new(
        FakeTourAgent::new()
            .with_terminal("t3", Some(PR), AgentStatus::Idle)
            .after_a_send_looks_report(&[AgentStatus::Working, AgentStatus::Failed]),
    );

    let (refreshed, _) = refresh(&fake, PR);

    let error = format!("{:#}", refreshed.unwrap_err());
    assert!(error.contains("t3") && error.contains("failed"), "{error}");
}

#[test]
fn a_pr_with_no_agent_gets_one_started_whose_first_task_is_the_refresh() {
    let fake = Arc::new(
        FakeTourAgent::new().after_a_send_looks_report(&[AgentStatus::Working, AgentStatus::Idle]),
    );

    let (refreshed, _) = refresh(&fake, PR);

    refreshed.unwrap();
    let calls = fake.calls();
    let [TourAgentCall::Start { first_task, .. }] = calls.as_slice() else {
        panic!("one start and nothing else, got {calls:?}");
    };
    assert!(
        first_task.starts_with(&format!("/tour {PR}")),
        "{first_task}"
    );
    assert!(first_task.contains("Refresh"), "{first_task}");
    assert!(!first_task.contains("subagent"), "{first_task}");
}

#[test]
fn several_agents_are_for_the_reviewer_to_pick_from_so_nothing_is_typed() {
    let fake = Arc::new(
        FakeTourAgent::new()
            .claude_in("t1", "/work/shop")
            .claude_in("t2", "/work/shop"),
    );

    let (refreshed, _) = refresh(&fake, LOCAL);

    let error = format!("{:#}", refreshed.unwrap_err());
    assert!(error.contains("Pick an agent"), "{error}");
    assert_eq!(fake.calls(), vec![]);
}

#[test]
fn a_local_reviews_agent_is_told_not_to_change_the_code_while_it_refreshes() {
    let fake = Arc::new(
        FakeTourAgent::new()
            .claude_in("t1", "/work/shop")
            .after_a_send_looks_report(&[AgentStatus::Working, AgentStatus::Idle]),
    );
    fake.set_status("t1", AgentStatus::Idle);

    let (refreshed, _) = refresh(&fake, LOCAL);

    refreshed.unwrap();
    let text = sent_text(&fake);
    assert!(text.contains("local key"), "{text}");
    assert!(text.contains("Do not change the code"), "{text}");
}

#[test]
fn with_the_agent_link_off_the_refresh_fails_and_says_why() {
    let fake = Arc::new(
        FakeTourAgent::new().without_herdr(guided_review::viewer::AgentLinkOff::ServerNotRunning),
    );

    let (refreshed, _) = refresh(&fake, PR);

    assert!(format!("{:#}", refreshed.unwrap_err()).contains("herdr server is not running"));
}

fn waits_long() -> Pace {
    Pace {
        poll: std::time::Duration::from_millis(5),
        free_agent: std::time::Duration::from_millis(400),
        start: std::time::Duration::from_millis(400),
    }
}

fn refresh_until_cancelled(fake: &Arc<FakeTourAgent>) -> anyhow::Result<()> {
    let agents = TourAgents::new(Arc::clone(fake) as Arc<dyn TourAgentPort>)
        .starting_in("/clones/shop".into());
    let subject: ReviewSubject = PR.parse().unwrap();
    let cancel = Cancel::default();
    let waiting = {
        let cancel = cancel.clone();
        let (ended, refreshed) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let refresh =
                TourAgentRefresh::new(agents, waits_long()).refresh(&subject, &cancel, &mut |_| {});
            ended.send(refresh).ok();
        });
        refreshed
    };
    std::thread::sleep(std::time::Duration::from_millis(30));
    cancel.cancel();
    waiting
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("the refresh still waits after the cancel")
}

#[test]
fn cancel_ends_the_wait_for_a_busy_agent_without_typing_into_it() {
    let fake = Arc::new(FakeTourAgent::new().with_terminal("t3", Some(PR), AgentStatus::Working));

    let refreshed = refresh_until_cancelled(&fake);

    assert!(refreshed.unwrap_err().is::<RefreshCancelled>());
    assert_eq!(fake.calls(), vec![]);
}

#[test]
fn cancel_ends_the_wait_for_the_agents_turn_and_leaves_the_agent_alone() {
    let fake = Arc::new(
        FakeTourAgent::new()
            .with_terminal("t3", Some(PR), AgentStatus::Idle)
            .after_a_send_looks_report(&[AgentStatus::Working]),
    );

    let refreshed = refresh_until_cancelled(&fake);

    assert!(refreshed.unwrap_err().is::<RefreshCancelled>());
    assert_eq!(fake.calls().len(), 1, "{:?}", fake.calls());
}
