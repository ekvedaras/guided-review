use crate::support::FixtureRepo;
use crate::support::app_path::AppPath;
use crate::support::fake_ports::{FakeGitHub, GitHubCall, GitHubFault, SentLog};
use guided_review::local_git::LocalGit;
use guided_review::pull_request::{PullRequestUrl, RepoName};
use guided_review::tour::Tour;
use guided_review::viewer::{
    ApproveWarning, FinishRefusal, FinishResult, Review, ReviewStateFile, ReviewVerdict,
};
use serde_json::json;

const PR: &str = "https://github.com/acme/shop/pull/7";

/// Three steps' worth of files, a tour that shows two of them, and one change no step shows.
struct Pull {
    repo: FixtureRepo,
    base: String,
    head: String,
    sent: SentLog,
}

impl Pull {
    fn new() -> Self {
        let repo = FixtureRepo::cloned_from("acme/shop");
        for name in ["a.txt", "b.txt", "c.txt"] {
            repo.write(name, "one\n");
        }
        let base = repo.commit("base");
        for name in ["a.txt", "b.txt", "c.txt"] {
            repo.write(name, "two\n");
        }
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

    fn open(&self) -> Review {
        let tour: Tour = serde_json::from_value(json!({
            "pr": PR,
            "base": self.base,
            "head": self.head,
            "title": "Letters",
            "summary": [],
            "steps": [
                {"id": "a", "title": "A", "why": "because", "pieces": [{"file": "a.txt"}], "children": []},
                {"id": "b", "title": "B", "why": "because", "pieces": [{"file": "b.txt"}], "children": []},
            ],
        }))
        .unwrap();
        let source = LocalGit::find(&RepoName::new("acme", "shop"), self.repo.path())
            .unwrap()
            .unwrap();
        let pr: PullRequestUrl = PR.parse().unwrap();
        Review::open(
            tour,
            &source,
            ReviewStateFile::for_pr(self.repo.home(), &pr),
        )
        .unwrap()
    }
}

#[test]
fn approve_warns_with_the_unread_steps_and_unread_safety_net_pieces() {
    let pull = Pull::new();
    let mut review = pull.open();
    review.tick(&"a".into()).unwrap();

    assert_eq!(
        review.approve_warning(),
        Some(ApproveWarning {
            unread_steps: 1,
            unread_safety_net_pieces: 1
        })
    );
}

#[test]
fn nothing_unread_means_no_warning() {
    let pull = Pull::new();
    let tour: Tour = serde_json::from_value(json!({
        "pr": PR, "base": pull.base, "head": pull.head, "title": "Letters", "summary": [],
        "steps": [{"id": "all", "title": "All", "why": "because", "pieces": [{"file": "a.txt"}, {"file": "b.txt"}, {"file": "c.txt"}], "children": []}],
    }))
    .unwrap();
    let source = LocalGit::find(&RepoName::new("acme", "shop"), pull.repo.path())
        .unwrap()
        .unwrap();
    let pr: PullRequestUrl = PR.parse().unwrap();
    let mut review = Review::open(
        tour,
        &source,
        ReviewStateFile::for_pr(pull.repo.home(), &pr),
    )
    .unwrap();
    review.tick(&"all".into()).unwrap();

    assert_eq!(review.approve_warning(), None);
}

#[test]
fn approve_and_request_changes_show_only_on_a_pr_someone_else_wrote() {
    let pull = Pull::new();
    let mut review = pull.open();
    assert!(!review.can_approve_or_request_changes(), "not known yet");

    review.pull_viewer_authorship(&pull.github()).unwrap();
    assert!(review.can_approve_or_request_changes());

    review
        .pull_viewer_authorship(&pull.github().with_viewer_as_pull_author())
        .unwrap();
    assert!(!review.can_approve_or_request_changes());
}

#[test]
fn finishing_sends_the_verdict_and_body() {
    let pull = Pull::new();
    let github = pull.github();
    let mut review = pull.open();
    review.pull_viewer_authorship(&github).unwrap();

    let result = review.finish_review(ReviewVerdict::Approve, "ship it", &github);

    assert_eq!(result, FinishResult::Sent);
    assert!(pull.sent.github_calls().contains(&GitHubCall::Submit {
        verdict: ReviewVerdict::Approve,
        body: "ship it".into()
    }));
}

#[test]
fn approving_or_requesting_changes_on_your_own_pr_is_refused_before_anything_is_sent() {
    let pull = Pull::new();
    let github = pull.github().with_viewer_as_pull_author();
    let mut review = pull.open();
    review.pull_viewer_authorship(&github).unwrap();

    for verdict in [ReviewVerdict::Approve, ReviewVerdict::RequestChanges] {
        assert_eq!(
            review.finish_review(verdict, "text", &github),
            FinishResult::Refused(FinishRefusal::OwnPullRequest)
        );
    }
    assert!(
        !pull
            .sent
            .github_calls()
            .iter()
            .any(|call| matches!(call, GitHubCall::Submit { .. }))
    );
}

#[test]
fn a_comment_review_with_no_text_needs_a_pending_review_to_send() {
    let pull = Pull::new();
    let mut review = pull.open();

    let nothing = review.finish_review(ReviewVerdict::Comment, "  ", &pull.github());
    assert_eq!(
        nothing,
        FinishResult::Refused(FinishRefusal::BodyNeeded(ReviewVerdict::Comment))
    );

    let github = pull.github().with_web_pending_review();
    review.pull_pending_review(&github).unwrap();
    assert_eq!(
        review.finish_review(ReviewVerdict::Comment, "", &github),
        FinishResult::Sent
    );
}

#[test]
fn requesting_changes_needs_text() {
    let pull = Pull::new();
    let github = pull.github().with_web_pending_review();
    let mut review = pull.open();
    review.pull_viewer_authorship(&github).unwrap();

    assert_eq!(
        review.finish_review(ReviewVerdict::RequestChanges, "", &github),
        FinishResult::Refused(FinishRefusal::BodyNeeded(ReviewVerdict::RequestChanges))
    );
}

#[test]
fn a_failed_finish_says_why() {
    let pull = Pull::new();
    let github = pull.github();
    let mut review = pull.open();
    review.pull_viewer_authorship(&github).unwrap();
    github.inject(GitHubFault::ConnectionLost);

    assert_eq!(
        review.finish_review(ReviewVerdict::Approve, "", &github),
        FinishResult::Failed("network is down".into())
    );
}

#[test]
fn finishing_ends_the_pending_review() {
    let pull = Pull::new();
    let github = pull.github().with_web_pending_review();
    let mut review = pull.open();
    review.pull_pending_review(&github).unwrap();
    assert!(!review.can_comment_now());

    review.finish_review(ReviewVerdict::Comment, "done", &github);

    assert!(review.can_comment_now());
}

#[test]
fn the_review_body_being_written_is_saved_and_goes_when_the_review_is_sent() {
    let pull = Pull::new();
    let github = pull.github();
    let mut review = pull.open();
    review.pull_viewer_authorship(&github).unwrap();
    review.edit_finish_body("looks good so far").unwrap();

    assert_eq!(pull.open().finish_body(), "looks good so far");

    let result = review.finish_review(ReviewVerdict::Comment, "looks good so far", &github);

    assert_eq!(result, FinishResult::Sent);
    assert_eq!(pull.open().finish_body(), "");
}

#[test]
fn a_refused_finish_keeps_the_body() {
    let pull = Pull::new();
    let github = pull.github().with_viewer_as_pull_author();
    let mut review = pull.open();
    review.pull_viewer_authorship(&github).unwrap();
    review.edit_finish_body("not yet").unwrap();

    let result = review.finish_review(ReviewVerdict::Approve, "not yet", &github);

    assert!(matches!(result, FinishResult::Refused(_)));
    assert_eq!(pull.open().finish_body(), "not yet");
}
