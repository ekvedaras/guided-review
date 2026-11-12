use crate::support::FixtureRepo;
use crate::support::fake_ports::{FakeGitHub, GitHubFault, SentLog};
use crate::support::fake_tour_agent::FakeTourAgent;
use crate::support::work::drive_without_agents;
use guided_review::local_git::LocalGit;
use guided_review::pull_request::RepoName;
use guided_review::tour::Tour;
use guided_review::viewer::{
    Problem, Review, ReviewStateFile, TourAgents, Work, WorkKind, WorkPorts,
};
use serde_json::json;
use std::sync::Arc;
use std::time::Instant;

const PR: &str = "https://github.com/acme/shop/pull/7";

struct Pull {
    repo: FixtureRepo,
    base: String,
    head: String,
    sent: SentLog,
}

impl Pull {
    fn new() -> Self {
        let repo = FixtureRepo::cloned_from("acme/shop");
        repo.write("a.txt", "old\n");
        let base = repo.commit("base");
        repo.write("a.txt", "new\n");
        let head = repo.commit("head");
        Self {
            repo,
            base,
            head,
            sent: SentLog::default(),
        }
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

    fn github(&self) -> FakeGitHub {
        FakeGitHub::with_pull("main-tip", &self.head, &self.sent)
    }

    fn land(&self, review: &mut Review, work: Work, github: &FakeGitHub) {
        let (source, agents) = (
            self.source(),
            TourAgents::new(Arc::new(FakeTourAgent::new())),
        );
        let ports = WorkPorts {
            github,
            source: &source,
            agents: &agents,
        };
        review.land(work.run(&ports));
    }
}

fn kinds_of(due: &[Work]) -> Vec<WorkKind> {
    due.iter().map(Work::kind).collect()
}

#[test]
fn a_window_coming_to_the_front_reads_github_and_the_clone_and_sends_what_waits_in_this_order() {
    let pull = Pull::new();
    let mut review = pull.open();

    review.pull_from_github();

    assert_eq!(
        kinds_of(&review.due_work(Instant::now())),
        [
            WorkKind::CheckPrHead,
            WorkKind::CheckWorkingCopy,
            WorkKind::PullPrDescription,
            WorkKind::SyncViewed,
            WorkKind::PullThreads,
            WorkKind::PullPendingReview,
            WorkKind::PullLastReview,
        ],
        "an empty queue sends nothing"
    );
}

#[test]
fn nothing_is_due_until_something_asks_for_it() {
    let pull = Pull::new();
    let mut review = pull.open();

    assert!(review.due_work(Instant::now()).is_empty());
    assert_eq!(review.next_due(), None);
}

#[test]
fn a_read_asked_again_while_one_runs_starts_and_the_older_answer_is_dropped() {
    let pull = Pull::new();
    let mut review = pull.open();
    let now = Instant::now();
    review.pull_from_github();
    let older = review
        .due_work(now)
        .into_iter()
        .find(|work| work.kind() == WorkKind::CheckPrHead)
        .unwrap();
    let offline = pull.github();
    offline.inject(GitHubFault::ReadsFail);

    review.pull_from_github();
    let newer = review
        .due_work(now)
        .into_iter()
        .find(|work| work.kind() == WorkKind::CheckPrHead)
        .unwrap();
    pull.land(&mut review, newer, &pull.github());
    pull.land(&mut review, older, &offline);

    assert_eq!(
        review.old_head(),
        None,
        "the newer read found the tour's head"
    );
    assert!(
        !review
            .problems()
            .iter()
            .any(|problem| matches!(problem, Problem::NotRead(..))),
        "{:?}",
        review.problems()
    );
}

#[test]
fn a_finished_review_reads_the_last_review_again() {
    let pull = Pull::new();
    let mut review = pull.open();
    review.viewer_authorship_is(false);
    let github = pull.github();
    let request = review
        .finish_review_request(guided_review::viewer::ReviewVerdict::Approve, "")
        .unwrap();
    review.apply_finish(request.run(&github));

    let ran = drive_without_agents(&mut review, &github, &pull.source(), Instant::now());

    assert_eq!(ran, [WorkKind::PullLastReview]);
}

#[test]
fn a_review_reopened_in_place_of_another_lands_the_work_the_other_started() {
    let pull = Pull::new();
    let mut review = pull.open();
    let now = Instant::now();
    review.tick(&"a".into()).unwrap();
    review.pull_from_github();
    let sync = review
        .due_work(now)
        .into_iter()
        .find(|work| work.kind() == WorkKind::SyncViewed)
        .unwrap();

    let mut reopened = pull.open();
    reopened.carry_on_from(review);
    reopened.pull_from_github();

    assert!(
        !kinds_of(&reopened.due_work(now)).contains(&WorkKind::SyncViewed),
        "the sync the other started still runs"
    );
    pull.land(&mut reopened, sync, &pull.github());
    assert!(kinds_of(&reopened.due_work(now)).contains(&WorkKind::SyncViewed));
}
