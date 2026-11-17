use std::time::{Duration, Instant};

use crate::support::FixtureRepo;
use crate::support::fake_ports::{FakeGitHub, GitHubFault, ReadFailure, SentLog};
use crate::support::work::drive_without_agents;
use guided_review::local_git::LocalGit;
use guided_review::pull_request::RepoName;
use guided_review::tour::GuidedTour;
use guided_review::viewer::{Checks, PrStatus, Problem, Read, Readiness, Review, ReviewStateFile};
use serde_json::json;

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
        repo.write("a.txt", "one\n");
        let base = repo.commit("base");
        repo.write("a.txt", "two\n");
        let head = repo.commit("head");
        Self {
            repo,
            base,
            head,
            sent: SentLog::default(),
        }
    }

    fn github(&self) -> FakeGitHub {
        FakeGitHub::with_pull("main-tip", &self.head, &self.sent)
    }

    fn source(&self) -> LocalGit {
        LocalGit::find(&RepoName::new("acme", "shop"), self.repo.path())
            .unwrap()
            .unwrap()
    }

    fn open(&self) -> Review {
        let tour: GuidedTour = serde_json::from_value(json!({
            "pr": PR,
            "base": self.base,
            "head": self.head,
            "title": "Letters",
            "summary": [],
            "steps": [
                {"id": "a", "title": "A", "why": "because", "pieces": [{"file": "a.txt"}], "children": []},
            ],
        }))
        .unwrap();
        let state_file = ReviewStateFile::for_subject(&self.repo.folders(), &PR.parse().unwrap());
        Review::open(tour, &self.source(), state_file).unwrap()
    }

    fn read(&self, review: &mut Review, github: &FakeGitHub, now: Instant) {
        review.pull_from_github();
        drive_without_agents(review, github, &self.source(), now);
    }
}

fn status(readiness: Readiness, checks: Checks) -> PrStatus {
    PrStatus {
        readiness,
        checks,
        viewer_may_change_readiness: true,
    }
}

#[test]
fn the_status_is_unknown_until_github_is_read_and_then_what_github_says() {
    let pull = Pull::new();
    let mut review = pull.open();
    let github = pull
        .github()
        .with_status(status(Readiness::Draft, Checks::Failing));
    assert_eq!(review.pr_status(), None);

    pull.read(&mut review, &github, Instant::now());

    assert_eq!(
        review.pr_status(),
        Some(&status(Readiness::Draft, Checks::Failing))
    );
}

#[test]
fn a_rollup_state_is_one_of_four_words() {
    assert_eq!(Checks::from_github_state(None), Checks::NoneRun);
    assert_eq!(Checks::from_github_state(Some("SUCCESS")), Checks::Passing);
    assert_eq!(Checks::from_github_state(Some("FAILURE")), Checks::Failing);
    assert_eq!(Checks::from_github_state(Some("ERROR")), Checks::Failing);
    assert_eq!(Checks::from_github_state(Some("PENDING")), Checks::Running);
    assert_eq!(Checks::from_github_state(Some("EXPECTED")), Checks::Running);
}

fn failing(pull: &Pull, failure: ReadFailure) -> FakeGitHub {
    let github = pull.github();
    github.inject(GitHubFault::ReadsFailWith(failure));
    github
}

#[test]
fn a_status_that_cannot_be_read_while_github_does_not_answer_is_one_more_thing_that_may_be_old() {
    let pull = Pull::new();
    let mut review = pull.open();
    let github = failing(&pull, ReadFailure::Status(503));

    pull.read(&mut review, &github, Instant::now());

    let problems = review.problems();
    assert_eq!(problems.len(), 1, "{problems:?}");
    let detail = problems[0].detail().unwrap();
    assert!(
        detail.contains("whether the PR is a draft and its checks"),
        "{detail}"
    );
}

#[test]
fn the_status_last_read_stays_while_github_does_not_answer_and_is_read_again_when_it_does() {
    let pull = Pull::new();
    let mut review = pull.open();
    let github = pull
        .github()
        .with_status(status(Readiness::Draft, Checks::Passing));
    let now = Instant::now();
    pull.read(&mut review, &github, now);
    github.inject(GitHubFault::ReadsFailWith(ReadFailure::Status(503)));

    pull.read(&mut review, &github, now + Duration::from_secs(1));
    assert_eq!(
        review.pr_status(),
        Some(&status(Readiness::Draft, Checks::Passing))
    );
    assert_eq!(review.problems().len(), 1);

    github.heal(&GitHubFault::ReadsFailWith(ReadFailure::Status(503)));
    github.set_status(status(Readiness::Ready, Checks::Passing));
    drive_without_agents(
        &mut review,
        &github,
        &pull.source(),
        now + Duration::from_secs(40),
    );

    assert_eq!(
        review.pr_status(),
        Some(&status(Readiness::Ready, Checks::Passing))
    );
    assert_eq!(review.problems(), []);
}

#[test]
fn a_refusal_to_read_the_status_is_a_problem_of_its_own_that_is_not_tried_again() {
    let pull = Pull::new();
    let mut review = pull.open();
    let github = failing(&pull, ReadFailure::Status(404));

    pull.read(&mut review, &github, Instant::now());

    assert!(
        review.problems().contains(&Problem::NotRead(
            Read::PrStatus,
            "http status: 404".to_owned()
        )),
        "{:?}",
        review.problems()
    );
    assert_eq!(review.next_due(), None);
}

#[test]
fn checks_that_are_running_are_read_again_every_30_seconds_until_they_are_done() {
    let pull = Pull::new();
    let mut review = pull.open();
    let github = pull
        .github()
        .with_status(status(Readiness::Ready, Checks::Running));
    let mut now = Instant::now();
    pull.read(&mut review, &github, now);

    let mut waits = Vec::new();
    for _ in 0..2 {
        let due = review.next_due().unwrap();
        waits.push(due - now);
        now = due;
        drive_without_agents(&mut review, &github, &pull.source(), now);
    }
    github.set_status(status(Readiness::Ready, Checks::Failing));
    let due = review.next_due().unwrap();
    drive_without_agents(&mut review, &github, &pull.source(), due);

    assert_eq!(
        waits.iter().map(Duration::as_secs).collect::<Vec<_>>(),
        [30, 30]
    );
    assert_eq!(
        review.pr_status().map(|status| status.checks),
        Some(Checks::Failing)
    );
    assert_eq!(review.next_due(), None);
}

#[test]
fn checks_that_are_not_running_are_not_read_again_by_themselves() {
    for checks in [Checks::NoneRun, Checks::Passing, Checks::Failing] {
        let pull = Pull::new();
        let mut review = pull.open();
        let github = pull.github().with_status(status(Readiness::Ready, checks));

        pull.read(&mut review, &github, Instant::now());

        assert_eq!(review.next_due(), None, "{checks:?}");
    }
}
