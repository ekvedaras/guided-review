use std::time::Instant;

use crate::support::FixtureRepo;
use crate::support::app_path::AppPath;
use crate::support::fake_ports::{FakeHost, HostCall, SentLog};
use crate::support::work::drive_without_agents;
use guided_review::local_git::LocalGit;
use guided_review::pull_request::{PullRequestUrl, RepoName, Sha};
use guided_review::review_subject::ReviewSubject;
use guided_review::tour::{BasicTour, TourPin};
use guided_review::viewer::{
    FinishResult, Reaction, ReactionChange, Review, ReviewOutcome, ReviewStateFile, ReviewVerdict,
    SubmittedReview,
};

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

    fn host(&self) -> FakeHost {
        FakeHost::with_pull("main-tip", &self.head, &self.sent)
    }

    fn source(&self) -> LocalGit {
        LocalGit::find(&RepoName::new("acme", "shop"), self.repo.path())
            .unwrap()
            .unwrap()
    }

    fn open(&self) -> Review {
        let tour = BasicTour::new(
            TourPin {
                version: 1,
                pr: PR.to_owned(),
                local: None,
                repo: None,
                host: None,
                base: Sha::new(&self.base),
                head: Sha::new(&self.head),
                previous_head: None,
            },
            "Letters".to_owned(),
        );
        let pr: PullRequestUrl = PR.parse().unwrap();
        Review::open_basic(
            tour,
            &self.source(),
            ReviewStateFile::for_subject(&self.repo.folders(), &ReviewSubject::from(pr)),
            crate::support::fake_ports::every_capability(),
        )
        .unwrap()
    }

    fn eyes_changes(&self) -> Vec<ReactionChange> {
        self.sent
            .host_calls()
            .into_iter()
            .filter_map(|call| match call {
                HostCall::SetPullRequestReaction {
                    reaction: Reaction::Eyes,
                    change,
                    ..
                } => Some(change),
                _ => None,
            })
            .collect()
    }
}

#[test]
fn a_basic_review_puts_eyes_on_the_pr_and_takes_them_off_when_the_review_is_sent() {
    let pull = Pull::new();
    let host = pull.host();
    let mut review = pull.open();
    review.pull_from_host();
    drive_without_agents(&mut review, &host, &pull.source(), Instant::now());
    assert_eq!(pull.eyes_changes(), vec![ReactionChange::Add]);

    review.finish_review(ReviewVerdict::Approve, "ship it", &host);
    drive_without_agents(&mut review, &host, &pull.source(), Instant::now());

    assert_eq!(
        pull.eyes_changes(),
        vec![ReactionChange::Add, ReactionChange::Remove]
    );
}

#[test]
fn a_basic_review_is_finished_with_a_verdict_and_a_body() {
    let pull = Pull::new();
    let host = pull.host();
    let mut review = pull.open();
    review.pull_viewer_authorship(&host).unwrap();

    let result = review.finish_review(ReviewVerdict::Approve, "ship it", &host);

    assert_eq!(result, FinishResult::Sent);
    assert!(pull.sent.host_calls().contains(&HostCall::Submit {
        verdict: ReviewVerdict::Approve,
        body: "ship it".into()
    }));
}

#[test]
fn a_review_sent_on_the_web_is_the_last_review_of_a_basic_review() {
    let pull = Pull::new();
    let sent = SubmittedReview {
        outcome: ReviewOutcome::ChangesRequested,
        body: "first look".to_owned(),
        submitted_at: "2026-11-01T08:00:00Z".to_owned(),
        commit: Sha::new(&pull.head),
    };
    let host = pull.host().with_last_review(sent.clone());
    let mut review = pull.open();

    review.pull_last_review(&host).unwrap();

    assert_eq!(review.last_review(), Some(&sent));
}
