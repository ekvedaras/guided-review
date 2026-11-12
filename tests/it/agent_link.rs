use crate::support::FixtureRepo;
use crate::support::fake_ports::{FakeGitHub, SentLog};
use crate::support::fake_tour_agent::FakeTourAgent;
use crate::support::work::{drive_until_idle, kinds};
use guided_review::local_git::LocalGit;
use guided_review::pull_request::{PullRequestUrl, RepoName};
use guided_review::tour::Tour;
use guided_review::viewer::{
    AGENT_LINK_RECHECK, AfterLanding, AgentLinkOff, AgentStatus, Review, ReviewStateFile,
    TerminalId, TourAgentPort, TourAgentState, TourAgents, WorkKind, WorkPorts,
};
use serde_json::json;
use std::sync::Arc;
use std::time::Instant;

const PR: &str = "https://github.com/acme/shop/pull/7";
const WRITER_STOPPED: &str = "cannot save terminals: registry writer stopped";

struct Pull {
    repo: FixtureRepo,
    base: String,
    head: String,
}

impl Pull {
    fn new() -> Self {
        let repo = FixtureRepo::cloned_from("acme/shop");
        repo.write("a.txt", "old\n");
        let base = repo.commit("base");
        repo.write("a.txt", "new\n");
        let head = repo.commit("head");
        Self { repo, base, head }
    }

    fn source(&self) -> LocalGit {
        LocalGit::find(&RepoName::new("acme", "shop"), self.repo.path())
            .unwrap()
            .unwrap()
    }

    fn open(&self) -> Review {
        let tour: Tour = serde_json::from_value(json!({
            "pr": PR,
            "base": self.base,
            "head": self.head,
            "title": "A change",
            "steps": [{"id": "a", "title": "A", "pieces": [{"file": "a.txt"}]}],
        }))
        .unwrap();
        let state_file = ReviewStateFile::for_pr(self.repo.home(), &PR.parse().unwrap());
        Review::open(tour, &self.source(), state_file).unwrap()
    }

    /// Runs what is due at `now` with these tour agents.
    fn drive_at(
        &self,
        review: &mut Review,
        agents: &TourAgents,
        now: Instant,
    ) -> Vec<(WorkKind, AfterLanding)> {
        let github = FakeGitHub::with_pull("main-tip", &self.head, &SentLog::default());
        let source = self.source();
        let ports = WorkPorts {
            github: &github,
            source: &source,
            agents,
        };
        drive_until_idle(review, &ports, now)
    }
}

fn pr() -> PullRequestUrl {
    PR.parse().unwrap()
}

fn agents_of(fake: &Arc<FakeTourAgent>) -> TourAgents {
    TourAgents::new(Arc::clone(fake) as Arc<dyn TourAgentPort>)
}

fn link_checks(ran: &[(WorkKind, AfterLanding)]) -> usize {
    kinds(ran)
        .into_iter()
        .filter(|kind| *kind == WorkKind::CheckAgentLink)
        .count()
}

/// A review whose follow of the tour agent just said the link is off, with the pulls from GitHub done.
fn review_with_link_off(pull: &Pull, agents: &TourAgents, start: Instant) -> Review {
    let mut review = pull.open();
    review.pull_from_github();
    pull.drive_at(&mut review, agents, start);
    review.tour_agent_is(&agents.lookup(&pr()));
    review
}

#[test]
fn a_missing_hc_says_so_and_names_no_command_line() {
    let fake = Arc::new(FakeTourAgent::new().without_herdr(AgentLinkOff::HcMissing));

    let line = agents_of(&fake).lookup(&pr()).to_string();

    assert!(line.contains("Tour agent is off"), "{line}");
    assert!(line.contains("`hc`") && line.contains("PATH"), "{line}");
}

#[test]
fn a_herdr_that_is_not_running_says_to_start_it() {
    let fake = Arc::new(FakeTourAgent::new().without_herdr(AgentLinkOff::ServerNotRunning));

    let line = agents_of(&fake).lookup(&pr()).to_string();

    assert!(line.contains("server is not running"), "{line}");
    assert!(line.contains("start herdr"), "{line}");
}

#[test]
fn a_herdr_that_runs_but_fails_shows_its_reason_and_offers_a_restart_with_a_warning() {
    let fake = Arc::new(FakeTourAgent::new().refusing(WRITER_STOPPED));

    let state = agents_of(&fake).lookup(&pr());

    assert_eq!(
        state,
        TourAgentState::LinkOff(AgentLinkOff::HerdrFailed(WRITER_STOPPED.to_owned()))
    );
    let line = state.to_string();
    assert!(line.contains("herdr is running but failed"), "{line}");
    assert!(line.contains(WRITER_STOPPED), "{line}");
    assert!(line.contains("hc server restart"), "{line}");
    assert!(line.contains("pause"), "{line}");
    assert!(!line.contains("ls --json"), "{line}");
}

#[test]
fn only_an_off_link_can_be_retried() {
    assert!(TourAgentState::LinkOff(AgentLinkOff::ServerNotRunning).can_retry());
    assert!(!TourAgentState::None.can_retry());
    assert!(!TourAgentState::Unknown.can_retry());
}

#[test]
fn a_failed_link_is_checked_again_after_the_interval_and_not_before() {
    let pull = Pull::new();
    let fake = Arc::new(FakeTourAgent::new().without_herdr(AgentLinkOff::ServerNotRunning));
    let agents = agents_of(&fake);
    let start = Instant::now();
    let mut review = review_with_link_off(&pull, &agents, start);
    pull.drive_at(&mut review, &agents, start);

    assert_eq!(review.next_due(), Some(start + AGENT_LINK_RECHECK));
    let too_soon = start + AGENT_LINK_RECHECK / 2;
    assert_eq!(
        link_checks(&pull.drive_at(&mut review, &agents, too_soon)),
        0
    );
    let ran = pull.drive_at(&mut review, &agents, start + AGENT_LINK_RECHECK);
    assert_eq!(link_checks(&ran), 1);
    assert_eq!(
        review.next_due(),
        Some(start + AGENT_LINK_RECHECK * 2),
        "still off, so it is checked again"
    );
}

#[test]
fn retry_checks_the_link_now() {
    let pull = Pull::new();
    let fake = Arc::new(FakeTourAgent::new().without_herdr(AgentLinkOff::ServerNotRunning));
    let agents = agents_of(&fake);
    let start = Instant::now();
    let mut review = review_with_link_off(&pull, &agents, start);
    pull.drive_at(&mut review, &agents, start);

    review.retry_agent_link();
    let ran = pull.drive_at(&mut review, &agents, start);

    assert_eq!(link_checks(&ran), 1);
}

#[test]
fn a_recovered_link_clears_the_message_and_turns_tour_agent_features_back_on() {
    let pull = Pull::new();
    let fake = Arc::new(
        FakeTourAgent::new()
            .with_terminal("t1", Some(PR), AgentStatus::Idle)
            .refusing(WRITER_STOPPED),
    );
    let agents = agents_of(&fake);
    let start = Instant::now();
    let mut review = review_with_link_off(&pull, &agents, start);
    pull.drive_at(&mut review, &agents, start);
    assert!(agents.show(&pr()).is_err());
    fake.recovers();

    let ran = pull.drive_at(&mut review, &agents, start + AGENT_LINK_RECHECK);

    let back = TourAgentState::Agent {
        terminal: TerminalId::new("t1"),
        status: AgentStatus::Idle,
    };
    assert_eq!(
        ran,
        [(WorkKind::CheckAgentLink, AfterLanding::TourAgentIs(back))]
    );
    assert_eq!(review.next_due(), None, "nothing is checked any more");
    agents.show(&pr()).unwrap();
}

#[test]
fn a_link_that_is_on_is_never_checked() {
    let pull = Pull::new();
    let fake = Arc::new(FakeTourAgent::new());
    let agents = agents_of(&fake);
    let start = Instant::now();
    let mut review = pull.open();
    review.tour_agent_is(&agents.lookup(&pr()));

    review.retry_agent_link();

    assert_eq!(link_checks(&pull.drive_at(&mut review, &agents, start)), 0);
    assert_eq!(review.next_due(), None);
}
