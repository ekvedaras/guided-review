use std::time::Instant;

use crate::support::FixtureRepo;
use crate::support::fake_ports::{FakeHost, SentLog};
use crate::support::work::drive_without_agents;
use guided_review::local_git::LocalGit;
use guided_review::pull_request::{PullRequestUrl, Sha};
use guided_review::review_subject::ReviewSubject;
use guided_review::tour::GuidedTour;
use guided_review::viewer::{
    Review, ReviewOutcome, ReviewRecord, ReviewStateFile, Reviewer, ReviewerLine, ReviewerStatus,
    ReviewersOnGitHub, Stance, StanceAge, Standing, SubmittedReview, Whose, YourReview,
};
use serde_json::json;

const PR: &str = "https://github.com/acme/shop/pull/7";

fn person(login: &str) -> Reviewer {
    Reviewer::person(login, Some(&format!("https://avatars.example/{login}")))
}

fn record(login: &str, outcome: ReviewOutcome) -> ReviewRecord {
    ReviewRecord {
        reviewer: person(login),
        outcome,
        commit: Sha::new("head"),
    }
}

fn on_github() -> ReviewersOnGitHub {
    ReviewersOnGitHub {
        viewer_login: "me".to_owned(),
        author_login: Some("author".to_owned()),
        latest: Vec::new(),
        verdicts: Vec::new(),
        requested: Vec::new(),
    }
}

fn stances(standings: &[Standing]) -> Vec<(String, Stance)> {
    standings
        .iter()
        .map(|standing| (standing.reviewer.name().to_owned(), standing.stance.clone()))
        .collect()
}

fn reviewed(outcome: ReviewOutcome) -> Stance {
    Stance::Reviewed {
        outcome,
        commit: Sha::new("head"),
    }
}

#[test]
fn the_viewer_comes_first_then_change_requests_approvals_comments_and_who_is_waited_on() {
    let standings = ReviewersOnGitHub {
        latest: vec![
            record("carol", ReviewOutcome::Commented),
            record("bob", ReviewOutcome::Approved),
            record("me", ReviewOutcome::Commented),
            record("alice", ReviewOutcome::ChangesRequested),
        ],
        requested: vec![Reviewer::team("backend", None)],
        ..on_github()
    }
    .standings();

    assert_eq!(
        stances(&standings),
        vec![
            ("me".to_owned(), reviewed(ReviewOutcome::Commented)),
            (
                "alice".to_owned(),
                reviewed(ReviewOutcome::ChangesRequested)
            ),
            ("bob".to_owned(), reviewed(ReviewOutcome::Approved)),
            ("carol".to_owned(), reviewed(ReviewOutcome::Commented)),
            ("backend".to_owned(), Stance::Requested),
        ]
    );
    assert!(standings[0].is_viewer());
    assert!(!standings[1].is_viewer());
}

#[test]
fn an_approval_outweighs_a_later_comment_of_the_same_reviewer() {
    let standings = ReviewersOnGitHub {
        latest: vec![record("bob", ReviewOutcome::Commented)],
        verdicts: vec![record("bob", ReviewOutcome::Approved)],
        ..on_github()
    }
    .standings();

    assert_eq!(
        stances(&standings),
        vec![("bob".to_owned(), reviewed(ReviewOutcome::Approved))]
    );
}

#[test]
fn the_pr_author_answering_threads_is_not_a_reviewer() {
    let standings = ReviewersOnGitHub {
        latest: vec![
            record("author", ReviewOutcome::Commented),
            record("bob", ReviewOutcome::Approved),
        ],
        ..on_github()
    }
    .standings();

    assert_eq!(
        stances(&standings),
        vec![("bob".to_owned(), reviewed(ReviewOutcome::Approved))]
    );
}

#[test]
fn a_reviewer_asked_again_keeps_the_review_that_counts_on_github() {
    let standings = ReviewersOnGitHub {
        verdicts: vec![record("bob", ReviewOutcome::Approved)],
        latest: vec![record("bob", ReviewOutcome::Approved)],
        requested: vec![person("bob"), person("dave")],
        ..on_github()
    }
    .standings();

    assert_eq!(
        stances(&standings),
        vec![
            ("bob".to_owned(), reviewed(ReviewOutcome::Approved)),
            ("dave".to_owned(), Stance::Requested),
        ]
    );
}

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

    fn source(&self) -> LocalGit {
        LocalGit::find(&crate::support::shop_on_github(), self.repo.path())
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
        let pr: PullRequestUrl = PR.parse().unwrap();
        Review::open(
            tour,
            &self.source(),
            ReviewStateFile::for_subject(&self.repo.folders(), &ReviewSubject::from(pr)),
            crate::support::fake_ports::every_capability(),
        )
        .unwrap()
    }

    fn reviewed_by(&self, standings: Vec<Standing>) -> FakeHost {
        FakeHost::with_pull("main-tip", &self.head, &self.sent).with_standings(standings)
    }
}

fn standing(login: &str, outcome: ReviewOutcome, commit: &str) -> Standing {
    let whose = if login == "me" {
        Whose::Viewers
    } else {
        Whose::Someone
    };
    Standing::of(
        person(login),
        whose,
        Stance::Reviewed {
            outcome,
            commit: Sha::new(commit),
        },
    )
}

fn sent(outcome: ReviewOutcome, commit: &str) -> SubmittedReview {
    SubmittedReview {
        outcome,
        body: "looks fine".to_owned(),
        submitted_at: "2026-11-18T09:00:00Z".to_owned(),
        commit: Sha::new(commit),
    }
}

fn your_review_after(pull: &Pull, host: &FakeHost) -> (Review, YourReview) {
    let mut review = pull.open();
    review.pull_from_host();
    drive_without_agents(&mut review, host, &pull.source(), Instant::now());
    let yours = review.your_review().unwrap();
    (review, yours)
}

#[test]
fn your_review_says_where_you_stand_and_when_you_sent_it() {
    let pull = Pull::new();
    let host = pull
        .reviewed_by(vec![standing("me", ReviewOutcome::Approved, &pull.base)])
        .with_last_review(sent(ReviewOutcome::Commented, &pull.base));

    let (_, yours) = your_review_after(&pull, &host);

    assert_eq!(yours.status, ReviewerStatus::Approved);
    assert_eq!(yours.title(), "You approved");
    assert_eq!(yours.since(), "Last review 2026-11-18");
    assert_eq!(yours.age, StanceAge::BeforeNewCommits);
    assert_eq!(yours.body(), "looks fine");
}

#[test]
fn marking_done_says_the_new_commits_were_checked() {
    let pull = Pull::new();
    let host = pull
        .reviewed_by(vec![standing("me", ReviewOutcome::Approved, &pull.base)])
        .with_last_review(sent(ReviewOutcome::Approved, &pull.base));
    let (mut review, _) = your_review_after(&pull, &host);

    review.mark_done().unwrap();
    let yours = review.your_review().unwrap();

    assert!(yours.is_marked_done);
    assert_eq!(
        yours.since(),
        "Last review 2026-11-18 · new commits checked"
    );
}

#[test]
fn your_review_falls_back_to_the_last_one_sent_when_github_lists_no_standing() {
    let pull = Pull::new();
    let host = pull
        .reviewed_by(Vec::new())
        .with_last_review(sent(ReviewOutcome::ChangesRequested, &pull.head));

    let (_, yours) = your_review_after(&pull, &host);

    assert_eq!(yours.title(), "You requested changes");
    assert_eq!(yours.since(), "Last review 2026-11-18");
    assert_eq!(yours.age, StanceAge::Current);
}

#[test]
fn a_review_asked_of_you_and_not_sent_yet_is_waiting() {
    let pull = Pull::new();
    let host = pull.reviewed_by(vec![Standing::of(
        person("me"),
        Whose::Viewers,
        Stance::Requested,
    )]);

    let (_, yours) = your_review_after(&pull, &host);

    assert_eq!(yours.title(), "Your review is requested");
    assert_eq!(yours.since(), "");
}

#[test]
fn the_sidebar_lines_name_the_viewer_you_and_say_which_reviews_came_before_new_commits() {
    let pull = Pull::new();
    let host = pull.reviewed_by(vec![
        standing("me", ReviewOutcome::Approved, &pull.base),
        standing("bob", ReviewOutcome::Approved, &pull.head),
    ]);
    let mut review = pull.open();

    review.pull_from_host();
    drive_without_agents(&mut review, &host, &pull.source(), Instant::now());

    let lines: Vec<(String, StanceAge)> = review
        .reviewer_lines()
        .iter()
        .map(|line: &ReviewerLine| (line.name.to_string(), line.age))
        .collect();
    assert_eq!(
        lines,
        vec![
            ("You".to_owned(), StanceAge::BeforeNewCommits),
            ("bob".to_owned(), StanceAge::Current),
        ]
    );
}
