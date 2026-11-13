use std::time::Instant;

use crate::support::FixtureRepo;
use crate::support::app_path::AppPath;
use crate::support::fake_ports::{FakeGitHub, GitHubCall, GitHubFault, SentLog};
use crate::support::work::drive_without_agents;
use guided_review::local_git::LocalGit;
use guided_review::pull_request::{PullRequestUrl, RepoName, Sha};
use guided_review::tour::Tour;
use guided_review::viewer::{
    Problem, Reaction, ReactionChange, Review, ReviewOutcome, ReviewStateFile, ReviewVerdict,
    SubmittedReview,
};
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
        let tour: Tour = serde_json::from_value(json!({
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
            ReviewStateFile::for_pr(self.repo.home(), &pr),
        )
        .unwrap()
    }

    /// The window came to the front: everything is read again and what waits is sent.
    fn come_to_front(&self, review: &mut Review, github: &FakeGitHub) {
        review.pull_from_github();
        drive_without_agents(review, github, &self.source(), Instant::now());
    }

    fn eyes_changes(&self) -> Vec<ReactionChange> {
        self.sent
            .github_calls()
            .into_iter()
            .filter_map(|call| match call {
                GitHubCall::SetPullRequestReaction {
                    reaction: Reaction::Eyes,
                    change,
                    ..
                } => Some(change),
                _ => None,
            })
            .collect()
    }
}

fn earlier_review(head: &str) -> SubmittedReview {
    SubmittedReview {
        outcome: ReviewOutcome::Commented,
        body: "first look".to_owned(),
        submitted_at: "2026-11-01T08:00:00Z".to_owned(),
        commit: Sha::new(head),
    }
}

#[test]
fn opening_someone_elses_pr_puts_eyes_on_it() {
    let pull = Pull::new();
    let github = pull.github();
    let mut review = pull.open();

    pull.come_to_front(&mut review, &github);

    assert_eq!(pull.eyes_changes(), vec![ReactionChange::Add]);
}

#[test]
fn the_eyes_go_on_once_however_often_the_window_comes_to_the_front() {
    let pull = Pull::new();
    let github = pull.github();
    let mut review = pull.open();

    pull.come_to_front(&mut review, &github);
    pull.come_to_front(&mut review, &github);

    assert_eq!(pull.eyes_changes(), vec![ReactionChange::Add]);
}

#[test]
fn the_viewers_own_pr_gets_no_eyes() {
    let pull = Pull::new();
    let github = pull.github().with_viewer_as_pull_author();
    let mut review = pull.open();

    pull.come_to_front(&mut review, &github);

    assert_eq!(pull.eyes_changes(), vec![]);
}

#[test]
fn a_review_sent_from_the_app_takes_the_eyes_off() {
    let pull = Pull::new();
    let github = pull.github();
    let mut review = pull.open();
    pull.come_to_front(&mut review, &github);

    review.finish_review(ReviewVerdict::Approve, "ship it", &github);
    drive_without_agents(&mut review, &github, &pull.source(), Instant::now());

    assert_eq!(
        pull.eyes_changes(),
        vec![ReactionChange::Add, ReactionChange::Remove]
    );
}

#[test]
fn a_review_sent_on_the_web_takes_the_eyes_off_once_it_is_read() {
    let pull = Pull::new();
    let github = pull.github();
    let mut review = pull.open();
    pull.come_to_front(&mut review, &github);

    let github = pull.github().with_last_review(earlier_review(&pull.head));
    pull.come_to_front(&mut review, &github);

    assert_eq!(
        pull.eyes_changes(),
        vec![ReactionChange::Add, ReactionChange::Remove]
    );
}

#[test]
fn a_review_sent_on_an_older_commit_before_the_pr_was_opened_leaves_the_eyes_on() {
    let pull = Pull::new();
    let github = pull.github().with_last_review(earlier_review(&pull.base));
    let mut review = pull.open();

    pull.come_to_front(&mut review, &github);
    pull.come_to_front(&mut review, &github);

    assert_eq!(pull.eyes_changes(), vec![ReactionChange::Add]);
}

#[test]
fn eyes_github_refused_are_a_problem_until_they_go_on_at_the_next_try() {
    let pull = Pull::new();
    let github = pull.github();
    github.inject(GitHubFault::ConnectionLost);
    let mut review = pull.open();

    pull.come_to_front(&mut review, &github);
    assert!(
        review
            .problems()
            .iter()
            .any(|problem| matches!(problem, Problem::ReviewingMarkNotChanged(_))),
        "{:?}",
        review.problems()
    );

    github.heal(&GitHubFault::ConnectionLost);
    pull.come_to_front(&mut review, &github);

    assert_eq!(
        pull.eyes_changes(),
        vec![ReactionChange::Add, ReactionChange::Add]
    );
    assert!(
        !review
            .problems()
            .iter()
            .any(|problem| matches!(problem, Problem::ReviewingMarkNotChanged(_)))
    );
}

#[test]
fn a_pr_reviewed_at_the_tours_head_gets_no_eyes_when_opened_again() {
    let pull = Pull::new();
    let github = pull.github().with_last_review(earlier_review(&pull.head));
    let mut review = pull.open();

    pull.come_to_front(&mut review, &github);

    assert_eq!(pull.eyes_changes(), vec![]);
}
