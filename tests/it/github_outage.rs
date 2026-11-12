use crate::support::FixtureRepo;
use crate::support::fake_ports::{FakeGitHub, GitHubFault, ReadFailure, SentLog};
use crate::support::work::drive_without_agents;
use guided_review::local_git::LocalGit;
use guided_review::pull_request::{RepoName, Sha};
use guided_review::tour::Tour;
use guided_review::viewer::{
    GitHubError, OutageCause, Problem, Read, Review, ReviewStateFile, ToastTone, announcements,
};
use serde_json::json;
use std::time::{Duration, Instant};

const PR: &str = "https://github.com/acme/shop/pull/7";

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

    fn failing(&self, failure: ReadFailure) -> FakeGitHub {
        let github = FakeGitHub::with_pull("main-tip", &self.head, &SentLog::default());
        github.inject(GitHubFault::ReadsFailWith(failure));
        github
    }

    fn read_everything_at(&self, review: &mut Review, github: &FakeGitHub, now: Instant) {
        review.pull_from_github();
        self.run_due_at(review, github, now);
    }

    fn run_due_at(&self, review: &mut Review, github: &FakeGitHub, now: Instant) {
        drive_without_agents(review, github, &self.source(), now);
    }
}

fn classified(error: ureq::Error) -> GitHubError {
    GitHubError::from(anyhow::Error::from(error))
}

#[test]
fn a_server_error_a_timeout_and_no_connection_are_an_outage_and_a_missing_page_is_not() {
    assert!(matches!(
        classified(ureq::Error::StatusCode(503)),
        GitHubError::NotAnswering(OutageCause::ServerError(503))
    ));
    assert!(matches!(
        classified(ureq::Error::Timeout(ureq::Timeout::Global)),
        GitHubError::NotAnswering(OutageCause::TimedOut)
    ));
    assert!(matches!(
        classified(ureq::Error::HostNotFound),
        GitHubError::NotAnswering(OutageCause::NoConnection)
    ));
    assert!(matches!(
        classified(ureq::Error::Io(
            std::io::ErrorKind::ConnectionRefused.into()
        )),
        GitHubError::NotAnswering(OutageCause::NoConnection)
    ));
    for status in [400, 401, 403, 404, 422] {
        assert!(
            matches!(
                classified(ureq::Error::StatusCode(status)),
                GitHubError::Other(_)
            ),
            "{status}"
        );
    }
}

#[test]
fn an_outage_is_known_through_the_context_a_read_adds() {
    let error = anyhow::Error::from(ureq::Error::StatusCode(502))
        .context("could not read the files of https://github.com/acme/shop/pull/7 from GitHub");

    assert!(matches!(
        GitHubError::from(error),
        GitHubError::NotAnswering(OutageCause::ServerError(502))
    ));
}

#[test]
fn many_reads_that_fail_because_github_does_not_answer_are_one_short_problem() {
    let pull = Pull::new();
    let mut review = pull.open();
    let github = pull.failing(ReadFailure::Status(503));

    pull.read_everything_at(&mut review, &github, Instant::now());

    let problems = review.problems();
    assert_eq!(problems.len(), 1, "{problems:?}");
    let shown = problems[0].to_string();
    assert_eq!(
        shown,
        "GitHub is not answering (503). Some things may be out of date; trying again in 30 s."
    );
    assert!(!shown.contains("https://"));
    let detail = problems[0].detail().unwrap();
    for affected in [
        "new commits",
        "the PR description",
        "comments",
        "viewed marks",
    ] {
        assert!(detail.contains(affected), "{detail}");
    }
}

#[test]
fn an_outage_is_one_note_that_leaves_by_itself_however_many_reads_fail_and_again() {
    let pull = Pull::new();
    let mut review = pull.open();
    let github = pull.failing(ReadFailure::Status(503));
    let now = Instant::now();

    pull.read_everything_at(&mut review, &github, now);
    let first = review.problems();
    let told = announcements(&[], &first);
    pull.run_due_at(&mut review, &github, now + Duration::from_secs(30));
    let second = review.problems();

    assert_eq!(told.len(), 1);
    assert_eq!(told[0].tone, ToastTone::Neutral);
    assert_eq!(told[0].text, "GitHub is not answering (503)");
    assert_eq!(announcements(&first, &second).len(), 0, "{second:?}");
}

#[test]
fn a_timeout_is_an_outage_too() {
    let pull = Pull::new();
    let mut review = pull.open();
    let github = pull.failing(ReadFailure::TimedOut);

    pull.read_everything_at(&mut review, &github, Instant::now());

    assert_eq!(
        review.problems()[0].to_string(),
        "GitHub is not answering (timed out). Some things may be out of date; trying again in 30 s."
    );
}

#[test]
fn reads_are_tried_again_further_apart_up_to_five_minutes() {
    let pull = Pull::new();
    let mut review = pull.open();
    let github = pull.failing(ReadFailure::Status(503));
    let mut now = Instant::now();
    pull.read_everything_at(&mut review, &github, now);

    let mut waits = Vec::new();
    for _ in 0..6 {
        let due = review.next_due().unwrap();
        waits.push(due - now);
        now = due;
        pull.run_due_at(&mut review, &github, now);
    }

    assert_eq!(
        waits.iter().map(Duration::as_secs).collect::<Vec<_>>(),
        [30, 60, 120, 240, 300, 300]
    );
}

#[test]
fn the_wait_shown_is_the_one_before_the_next_try() {
    let pull = Pull::new();
    let mut review = pull.open();
    let github = pull.failing(ReadFailure::Status(503));
    let now = Instant::now();
    pull.read_everything_at(&mut review, &github, now);

    pull.run_due_at(&mut review, &github, now + Duration::from_secs(30));

    assert!(
        review.problems()[0]
            .to_string()
            .ends_with("trying again in 1 min."),
        "{:?}",
        review.problems()
    );
}

#[test]
fn a_read_that_goes_through_ends_the_outage_and_says_github_is_back() {
    let pull = Pull::new();
    let mut review = pull.open();
    let github = pull.failing(ReadFailure::Status(503));
    let now = Instant::now();
    pull.read_everything_at(&mut review, &github, now);
    let during = review.problems();

    github.heal(&GitHubFault::ReadsFailWith(ReadFailure::Status(503)));
    pull.run_due_at(&mut review, &github, now + Duration::from_secs(30));

    assert_eq!(review.problems(), []);
    let told = announcements(&during, &review.problems());
    assert_eq!(told.len(), 1);
    assert_eq!(told[0].tone, ToastTone::Success);
    assert_eq!(told[0].text, "GitHub is back");
}

#[test]
fn the_outage_stays_while_one_of_its_reads_still_fails() {
    let pull = Pull::new();
    let mut review = pull.open();
    let github = pull.failing(ReadFailure::Status(503));
    pull.read_everything_at(&mut review, &github, Instant::now());

    review.pr_head_is(Sha::new(&pull.head));

    let problems = review.problems();
    assert_eq!(problems.len(), 1);
    assert!(!problems[0].detail().unwrap().contains("new commits"));
}

#[test]
fn a_missing_page_or_a_refusal_keeps_its_own_short_message_and_is_not_tried_again() {
    for status in [404, 403] {
        let pull = Pull::new();
        let mut review = pull.open();
        let github = pull.failing(ReadFailure::Status(status));

        pull.read_everything_at(&mut review, &github, Instant::now());

        let reason = format!("http status: {status}");
        let problems = review.problems();
        assert!(
            problems.contains(&Problem::NotRead(Read::PrHead, reason.clone())),
            "{problems:?}"
        );
        assert!(
            problems.contains(&Problem::ThreadsNotRead(reason.clone())),
            "{problems:?}"
        );
        for problem in &problems {
            let shown = problem.to_string();
            assert!(!shown.contains("https://"), "{shown}");
            assert!(!shown.contains("not answering"), "{shown}");
            assert!(shown.ends_with(&reason), "{shown}");
        }
        assert_eq!(review.next_due(), None);
    }
}

#[test]
fn a_problem_that_is_not_an_outage_is_told_in_its_own_note_that_stays() {
    let problem = Problem::NotRead(Read::PrHead, "http status: 404".into());

    let told = announcements(&[], std::slice::from_ref(&problem));

    assert_eq!(told.len(), 1);
    assert_eq!(told[0].tone, ToastTone::Danger);
    assert_eq!(told[0].text, problem.to_string());
}
