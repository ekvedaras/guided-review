use std::ops::ControlFlow;
use std::path::Path;
use std::sync::Arc;

use crate::support::fake_checkouts::FakeCheckouts;
use crate::support::fake_tour_agent::{FakeTourAgent, TourAgentCall};
use crate::support::folders_in;
use guided_review::prompt_safety::OUTSIDE_TEXT_IS_DATA;
use guided_review::pull_request::Sha;
use guided_review::review_subject::ReviewSubject;
use guided_review::tour::Tour;
use guided_review::viewer::{
    AgentStatus, ReviewAgents, SendOutcome, TerminalId, TourAgentPort, TourAgentState, TourAgents,
    TourKind,
};
use tempfile::TempDir;

const PR: &str = "https://github.com/acme/shop/pull/7";
const OTHER_PR: &str = "https://github.com/acme/shop/pull/8";

fn pr() -> ReviewSubject {
    PR.parse().unwrap()
}

struct Desk {
    fake: Arc<FakeTourAgent>,
    checkouts: FakeCheckouts,
    home: TempDir,
}

impl Desk {
    fn with(fake: FakeTourAgent) -> Self {
        Self {
            fake: Arc::new(fake),
            checkouts: FakeCheckouts::default().clean_at("/clones/shop", "other"),
            home: TempDir::new().unwrap(),
        }
    }

    fn with_checkouts(mut self, checkouts: FakeCheckouts) -> Self {
        self.checkouts = checkouts;
        self
    }

    fn review_agents(&self) -> ReviewAgents {
        ReviewAgents::new(
            Arc::clone(&self.fake) as Arc<dyn TourAgentPort>,
            Arc::new(self.checkouts.clone()),
            folders_in(self.home.path()),
        )
    }

    fn tour_agents(&self) -> TourAgents {
        TourAgents::new(Arc::clone(&self.fake) as Arc<dyn TourAgentPort>)
            .starting_in("/clones/shop".into())
    }

    fn start(&self, subject: &ReviewSubject) -> anyhow::Result<TerminalId> {
        self.start_with_tour_agent_in(subject, None)
    }

    fn start_with_tour_agent_in(
        &self,
        subject: &ReviewSubject,
        tour_agent_folder: Option<&str>,
    ) -> anyhow::Result<TerminalId> {
        self.review_agents().start(
            subject,
            tour_agent_folder.map(Path::new),
            Path::new("/clones/shop"),
            &Sha::new("abc123"),
            subject,
        )
    }
}

fn started_review(fake: &FakeTourAgent) -> (String, String, String, String) {
    let calls = fake.calls();
    let Some(TourAgentCall::StartReview {
        name,
        first_task,
        tools,
        denied_tools,
        ..
    }) = calls
        .iter()
        .find(|call| matches!(call, TourAgentCall::StartReview { .. }))
    else {
        panic!("a review agent was started, got {calls:?}");
    };
    (
        name.clone(),
        first_task.clone(),
        tools.clone(),
        denied_tools.clone(),
    )
}

fn started_in(fake: &FakeTourAgent) -> std::path::PathBuf {
    let calls = fake.calls();
    let [TourAgentCall::StartReview { folder, .. }] = calls.as_slice() else {
        panic!("one review start, got {calls:?}");
    };
    folder.clone()
}

#[test]
fn a_review_agent_starts_in_a_clean_tour_agent_folder_at_the_reviewed_head_and_is_told_to_work_there_as_it_is()
 {
    let desk = Desk::with(FakeTourAgent::new())
        .with_checkouts(FakeCheckouts::default().clean_at("/work/shop", "abc123"));

    let terminal = desk
        .start_with_tour_agent_in(&pr(), Some("/work/shop"))
        .unwrap();

    assert_eq!(started_in(&desk.fake), Path::new("/work/shop"));
    let (_, first_task, tools, _) = started_review(&desk.fake);
    assert!(
        first_task.contains("in this folder as it is"),
        "{first_task}"
    );
    assert!(!first_task.contains("EnterWorktree"), "{first_task}");
    assert!(tools.contains("Read(//work/shop/**)"), "{tools}");
    assert_eq!(desk.fake.review_agents_of(PR), [terminal.to_string()]);
}

fn assert_told_to_make_its_own_worktree(desk: &Desk) {
    assert_eq!(started_in(&desk.fake), Path::new("/clones/shop"));
    let (_, first_task, _, _) = started_review(&desk.fake);
    assert!(first_task.contains("EnterWorktree"), "{first_task}");
    assert!(
        first_task.contains("git checkout --detach abc123"),
        "{first_task}"
    );
    assert!(
        first_task.contains("never with `git worktree add`"),
        "{first_task}"
    );
    assert!(first_task.contains("ExitWorktree"), "{first_task}");
    assert!(first_task.contains("\"remove\""), "{first_task}");
    assert!(!first_task.contains("as it is"), "{first_task}");
}

#[test]
fn a_tour_agent_folder_on_another_commit_starts_the_agent_in_the_clone_to_make_its_own_worktree() {
    let desk = Desk::with(FakeTourAgent::new()).with_checkouts(
        FakeCheckouts::default()
            .clean_at("/clones/shop", "other")
            .clean_at("/work/shop", "def456"),
    );

    desk.start_with_tour_agent_in(&pr(), Some("/work/shop"))
        .unwrap();

    assert_told_to_make_its_own_worktree(&desk);
}

#[test]
fn a_dirty_tour_agent_folder_starts_the_agent_in_the_clone_to_make_its_own_worktree() {
    let desk = Desk::with(FakeTourAgent::new()).with_checkouts(
        FakeCheckouts::default()
            .clean_at("/clones/shop", "other")
            .dirty_at("/work/shop", "abc123"),
    );

    desk.start_with_tour_agent_in(&pr(), Some("/work/shop"))
        .unwrap();

    assert_told_to_make_its_own_worktree(&desk);
}

#[test]
fn no_tour_agent_starts_the_agent_in_the_clone_to_make_its_own_worktree() {
    let desk = Desk::with(FakeTourAgent::new());

    let terminal = desk.start(&pr()).unwrap();

    assert_told_to_make_its_own_worktree(&desk);
    assert_eq!(desk.fake.review_agents_of(PR), [terminal.to_string()]);
}

#[test]
fn an_agent_that_makes_its_own_worktree_is_allowed_to_enter_and_leave_it() {
    let desk = Desk::with(FakeTourAgent::new());

    desk.start(&pr()).unwrap();

    let (_, _, tools, _) = started_review(&desk.fake);
    assert!(tools.contains("EnterWorktree"), "{tools}");
    assert!(tools.contains("ExitWorktree"), "{tools}");
}

#[test]
fn the_review_agent_is_told_that_outside_text_is_data() {
    let desk = Desk::with(FakeTourAgent::new());

    desk.start(&pr()).unwrap();

    let (_, first_task, _, _) = started_review(&desk.fake);
    assert!(first_task.contains(OUTSIDE_TEXT_IS_DATA), "{first_task}");
}

#[test]
fn the_review_agent_is_told_to_read_its_brief_and_may_write_only_its_findings() {
    let desk = Desk::with(FakeTourAgent::new());

    desk.start(&pr()).unwrap();

    let (name, first_task, tools, denied) = started_review(&desk.fake);
    assert_eq!(name, "review-acme-shop-7");
    assert!(
        first_task.contains(&format!("`guided-review review-brief {PR}`")),
        "{first_task}"
    );
    let findings = Tour::findings_path_for(&folders_in(desk.home.path()), &pr());
    let findings = format!(
        "//{}",
        findings.display().to_string().trim_start_matches('/')
    );
    assert!(tools.contains(&format!("Write({findings})")), "{tools}");
    assert_eq!(tools.matches("Write(").count(), 1, "{tools}");
    assert!(denied.contains("Bash(git push:*)"), "{denied}");
}

#[test]
fn the_review_agent_runs_the_review_skill_when_the_reviewer_set_one() {
    let desk = Desk::with(FakeTourAgent::new());
    let folders = folders_in(desk.home.path());
    std::fs::create_dir_all(folders.config_file().parent().unwrap()).unwrap();
    std::fs::write(
        folders.config_file(),
        "review_instructions = { skill = \"my-review\" }\n",
    )
    .unwrap();

    desk.start(&pr()).unwrap();

    let (_, _, tools, _) = started_review(&desk.fake);
    assert!(tools.contains("Skill(my-review)"), "{tools}");
}

#[test]
fn a_clone_that_is_no_checkout_starts_no_agent() {
    let desk = Desk::with(FakeTourAgent::new()).with_checkouts(FakeCheckouts::default());

    let error = desk.start(&pr()).unwrap_err();

    assert!(format!("{error:#}").contains("no clone"));
    assert_eq!(desk.fake.calls(), vec![]);
}

#[test]
fn a_second_run_replaces_the_first_agent() {
    let desk = Desk::with(FakeTourAgent::new().review_agent("t5", PR, AgentStatus::Idle, 10));

    let terminal = desk.start(&pr()).unwrap();

    assert_eq!(desk.fake.review_agents_of(PR), [terminal.to_string()]);
    let calls = desk.fake.calls();
    assert_eq!(
        calls.first(),
        Some(&TourAgentCall::Kill(TerminalId::new("t5")))
    );
}

#[test]
fn herdr_being_off_starts_nothing() {
    let desk = Desk::with(
        FakeTourAgent::new().without_herdr(guided_review::viewer::AgentLinkOff::ServerNotRunning),
    );

    let error = desk.start(&pr()).unwrap_err();

    assert!(error.to_string().contains("not running"), "{error}");
    assert_eq!(desk.fake.calls(), vec![]);
}

fn five_working_tour_agents() -> FakeTourAgent {
    (1..=5).fold(FakeTourAgent::new(), |fake, n| {
        fake.app_started(
            &format!("t{n}"),
            &format!("https://github.com/acme/shop/pull/{}", n + 20),
            AgentStatus::Working,
            n,
        )
    })
}

#[test]
fn a_review_agent_shares_the_five_slots_and_ends_the_idle_tour_agent_used_longest_ago() {
    let desk = Desk::with(
        FakeTourAgent::new()
            .app_started(
                "t1",
                "https://github.com/acme/shop/pull/21",
                AgentStatus::Idle,
                5,
            )
            .app_started(
                "t2",
                "https://github.com/acme/shop/pull/22",
                AgentStatus::Idle,
                1,
            )
            .app_started(
                "t3",
                "https://github.com/acme/shop/pull/23",
                AgentStatus::Working,
                0,
            )
            .app_started(
                "t4",
                "https://github.com/acme/shop/pull/24",
                AgentStatus::Working,
                0,
            )
            .app_started(
                "t5",
                "https://github.com/acme/shop/pull/25",
                AgentStatus::Working,
                0,
            ),
    );

    desk.start(&pr()).unwrap();

    assert!(
        desk.fake
            .calls()
            .contains(&TourAgentCall::Kill(TerminalId::new("t2")))
    );
    assert!(
        !desk
            .fake
            .calls()
            .contains(&TourAgentCall::Kill(TerminalId::new("t1")))
    );
}

#[test]
fn with_five_busy_agents_no_review_agent_starts() {
    let desk = Desk::with(five_working_tour_agents());

    let error = desk.start(&pr()).unwrap_err();

    assert!(error.to_string().contains("none can be ended"), "{error}");
}

fn four_working_tour_agents(fake: FakeTourAgent) -> FakeTourAgent {
    (2..=5).fold(fake, |fake, n| {
        fake.app_started(
            &format!("t{n}"),
            &format!("https://github.com/acme/shop/pull/{}", n + 20),
            AgentStatus::Working,
            0,
        )
    })
}

#[test]
fn an_idle_review_agent_is_ended_like_an_idle_tour_agent_when_room_is_needed() {
    let desk = Desk::with(four_working_tour_agents(FakeTourAgent::new().review_agent(
        "t1",
        OTHER_PR,
        AgentStatus::Idle,
        0,
    )));
    desk.tour_agents()
        .start(&pr(), &pr(), TourKind::Guided)
        .unwrap();

    assert!(
        desk.fake
            .calls()
            .contains(&TourAgentCall::Kill(TerminalId::new("t1")))
    );
}

#[test]
fn the_idle_agent_used_longest_ago_is_ended_whether_it_reviews_or_writes_a_tour() {
    let desk = Desk::with(
        four_working_tour_agents(FakeTourAgent::new().review_agent(
            "t1",
            OTHER_PR,
            AgentStatus::Idle,
            9,
        ))
        .app_started(
            "t6",
            "https://github.com/acme/shop/pull/26",
            AgentStatus::Idle,
            1,
        ),
    );

    desk.tour_agents()
        .start(&pr(), &pr(), TourKind::Guided)
        .unwrap();

    let calls = desk.fake.calls();
    assert!(
        calls.contains(&TourAgentCall::Kill(TerminalId::new("t6"))),
        "{calls:?}"
    );
    assert!(
        !calls.contains(&TourAgentCall::Kill(TerminalId::new("t1"))),
        "{calls:?}"
    );
}

#[test]
fn the_review_agent_of_the_open_subject_is_not_ended_while_room_can_be_made_elsewhere() {
    let desk = Desk::with(
        four_working_tour_agents(FakeTourAgent::new().review_agent("t1", PR, AgentStatus::Idle, 0))
            .app_started(
                "t6",
                "https://github.com/acme/shop/pull/26",
                AgentStatus::Idle,
                9,
            ),
    );

    desk.tour_agents()
        .start(&OTHER_PR.parse().unwrap(), &pr(), TourKind::Guided)
        .unwrap();

    let calls = desk.fake.calls();
    assert!(
        !calls.contains(&TourAgentCall::Kill(TerminalId::new("t1"))),
        "{calls:?}"
    );
    assert!(
        calls.contains(&TourAgentCall::Kill(TerminalId::new("t6"))),
        "{calls:?}"
    );
}

#[test]
fn with_only_busy_agents_and_the_open_review_agent_a_start_is_refused_naming_them() {
    let desk = Desk::with(four_working_tour_agents(FakeTourAgent::new().review_agent(
        "t1",
        PR,
        AgentStatus::Idle,
        0,
    )));

    let error = desk
        .tour_agents()
        .start(&OTHER_PR.parse().unwrap(), &pr(), TourKind::Guided)
        .unwrap_err();

    assert!(
        error.to_string().contains(&format!("review of {PR}")),
        "{error}"
    );
}

#[test]
fn tour_agent_lookups_never_find_a_review_agent() {
    let desk = Desk::with(FakeTourAgent::new().review_agent("t5", PR, AgentStatus::Idle, 0));
    let tours = desk.tour_agents();

    assert_eq!(tours.lookup(&pr()), TourAgentState::None);
    assert_eq!(
        tours.send_if_idle(&pr(), "hello").unwrap(),
        SendOutcome::NoAgent
    );
    assert!(tours.show(&pr()).is_err());
    assert_eq!(desk.fake.calls(), vec![]);
}

#[test]
fn writing_a_tour_does_not_end_the_review_agent() {
    let desk = Desk::with(FakeTourAgent::new().review_agent("t5", PR, AgentStatus::Working, 0));

    desk.tour_agents().write_tour(&pr(), &pr()).unwrap();

    assert!(
        !desk
            .fake
            .calls()
            .contains(&TourAgentCall::Kill(TerminalId::new("t5")))
    );
    assert_eq!(desk.fake.review_agents_of(PR), ["t5"]);
}

#[test]
fn a_review_agent_is_found_by_its_label_only() {
    let desk = Desk::with(
        FakeTourAgent::new()
            .with_terminal("t3", Some(PR), AgentStatus::Idle)
            .review_agent("t5", PR, AgentStatus::Working, 0),
    );

    assert_eq!(
        desk.review_agents().lookup(&pr()),
        TourAgentState::Agent {
            terminal: TerminalId::new("t5"),
            status: AgentStatus::Working
        }
    );
    let nobody: ReviewSubject = OTHER_PR.parse().unwrap();
    assert_eq!(desk.review_agents().lookup(&nobody), TourAgentState::None);
}

#[test]
fn only_a_review_agent_the_app_started_is_listed_as_running() {
    let desk = Desk::with(
        FakeTourAgent::new()
            .app_started("t3", OTHER_PR, AgentStatus::Idle, 0)
            .review_agent("t5", PR, AgentStatus::Working, 0),
    );

    assert_eq!(desk.review_agents().running_subjects().unwrap(), [pr()]);
}

#[test]
fn the_review_agents_statuses_are_followed_from_its_state_now() {
    let desk = Desk::with(
        FakeTourAgent::new()
            .review_agent("t5", PR, AgentStatus::Working, 0)
            .then_reports(&[AgentStatus::Idle]),
    );
    let mut seen = Vec::new();

    desk.review_agents().follow(&pr(), &mut |state| {
        seen.push(state);
        ControlFlow::Continue(())
    });

    assert_eq!(
        seen,
        [
            TourAgentState::Agent {
                terminal: TerminalId::new("t5"),
                status: AgentStatus::Working
            },
            TourAgentState::Agent {
                terminal: TerminalId::new("t5"),
                status: AgentStatus::Idle
            },
        ]
    );
}

#[test]
fn showing_brings_up_the_review_agents_terminal_and_a_missing_one_is_said() {
    let desk = Desk::with(FakeTourAgent::new().review_agent("t5", PR, AgentStatus::Working, 0));

    desk.review_agents().show(&pr()).unwrap();
    let nobody: ReviewSubject = OTHER_PR.parse().unwrap();
    let error = desk.review_agents().show(&nobody).unwrap_err();

    assert_eq!(
        desk.fake.calls(),
        [TourAgentCall::Show(TerminalId::new("t5"))]
    );
    assert!(error.to_string().contains("no review agent"), "{error}");
}

#[test]
fn ending_the_review_ends_its_agent() {
    let desk = Desk::with(FakeTourAgent::new());
    let terminal = desk.start(&pr()).unwrap();

    desk.review_agents().end(&pr()).unwrap();

    assert!(desk.fake.calls().contains(&TourAgentCall::Kill(terminal)));
    assert_eq!(desk.fake.review_agents_of(PR), Vec::<String>::new());
}
