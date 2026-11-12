use crate::support::app_path::AppPath;
use crate::support::fake_ports::{FakeGitHub, GitHubCall, GitHubFault, SentLog};
use crate::support::{FixtureRepo, save_typing};
use guided_review::local_git::LocalGit;
use guided_review::pull_request::{PullRequestUrl, RepoName, Sha};
use guided_review::tour::Tour;
use guided_review::viewer::{
    ApproveWarning, FinishRefusal, FinishResult, Review, ReviewOutcome, ReviewStateFile,
    ReviewVerdict, SubmittedReview, TextBox,
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
fn finish_review_is_offered_on_ones_own_pr_only_with_comments_to_send() {
    let pull = Pull::new();
    let mut review = pull.open();
    assert!(!review.can_finish_review(), "not known yet");

    review
        .pull_viewer_authorship(&pull.github().with_viewer_as_pull_author())
        .unwrap();
    review.pending_review_is(false);
    assert!(
        !review.can_finish_review(),
        "nothing to send and no verdict to give"
    );

    review.pending_review_is(true);
    assert!(review.can_finish_review(), "the pending comments go out");
}

#[test]
fn finish_review_is_offered_on_someone_elses_pr_with_nothing_pending() {
    let pull = Pull::new();
    let mut review = pull.open();

    review.pull_viewer_authorship(&pull.github()).unwrap();
    review.pending_review_is(false);

    assert!(review.can_finish_review());
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
fn the_review_body_goes_when_the_review_is_sent() {
    let pull = Pull::new();
    let github = pull.github();
    let mut review = pull.open();
    review.pull_viewer_authorship(&github).unwrap();
    review
        .type_in(&TextBox::FinishBody, "looks good so far")
        .unwrap();
    save_typing(&mut review);

    let result = review.finish_review(ReviewVerdict::Comment, "looks good so far", &github);

    assert_eq!(result, FinishResult::Sent);
    assert_eq!(pull.open().text_of(&TextBox::FinishBody), None);
}

#[test]
fn a_refused_finish_keeps_the_body() {
    let pull = Pull::new();
    let github = pull.github().with_viewer_as_pull_author();
    let mut review = pull.open();
    review.pull_viewer_authorship(&github).unwrap();
    review.type_in(&TextBox::FinishBody, "not yet").unwrap();
    save_typing(&mut review);

    let result = review.finish_review(ReviewVerdict::Approve, "not yet", &github);

    assert!(matches!(result, FinishResult::Refused(_)));
    assert_eq!(pull.open().text_of(&TextBox::FinishBody), Some("not yet"));
}

fn web_review(outcome: ReviewOutcome, commit: &str) -> SubmittedReview {
    SubmittedReview {
        outcome,
        body: "needs a test".into(),
        submitted_at: "2026-11-05T13:35:33Z".into(),
        commit: Sha::new(commit),
    }
}

#[test]
fn a_body_of_many_lines_or_many_words_is_long_enough_to_clamp() {
    let with_body = |body: &str| SubmittedReview {
        body: body.into(),
        ..web_review(ReviewOutcome::Commented, "abc")
    };

    assert!(!with_body("needs a test").body_is_long());
    assert!(!with_body("one\ntwo\nthree\nfour").body_is_long());
    assert!(with_body("one\ntwo\nthree\nfour\nfive").body_is_long());
    assert!(with_body(&"word ".repeat(80)).body_is_long());
}

#[test]
fn the_headline_says_how_the_review_ended_when_and_whether_the_pr_moved_on() {
    let review = web_review(ReviewOutcome::ChangesRequested, "abc");

    assert_eq!(
        review.headline(false),
        "Your last review: Changes requested on 2026-11-05"
    );
    assert_eq!(
        review.headline(true),
        "Your last review: Changes requested on 2026-11-05, PR has new commits since"
    );
}

#[test]
fn a_review_never_sent_has_no_last_review() {
    let pull = Pull::new();
    let mut review = pull.open();

    review.pull_last_review(&pull.github()).unwrap();

    assert_eq!(review.last_review(), None);
}

#[test]
fn a_review_sent_from_the_web_is_the_last_review() {
    let pull = Pull::new();
    let sent = web_review(ReviewOutcome::ChangesRequested, &pull.head);
    let github = pull.github().with_last_review(sent.clone());
    let mut review = pull.open();

    review.pull_last_review(&github).unwrap();

    assert_eq!(review.last_review(), Some(&sent));
}

#[test]
fn the_last_review_is_old_once_the_pr_has_moved_past_its_commit() {
    let pull = Pull::new();
    let github = pull
        .github()
        .with_last_review(web_review(ReviewOutcome::Approved, "older-commit"));
    let mut review = pull.open();
    review.pull_last_review(&github).unwrap();
    assert!(!review.last_review_is_old(), "the PR head is not known yet");

    review.check_pr_head(&github).unwrap();

    assert!(review.last_review_is_old());
}

#[test]
fn the_last_review_is_not_old_while_the_pr_is_at_its_commit() {
    let pull = Pull::new();
    let github = pull
        .github()
        .with_last_review(web_review(ReviewOutcome::Approved, &pull.head));
    let mut review = pull.open();
    review.pull_last_review(&github).unwrap();

    review.check_pr_head(&github).unwrap();

    assert!(!review.last_review_is_old());
}

#[test]
fn finishing_makes_the_review_the_last_one_when_read_again() {
    let pull = Pull::new();
    let github = pull.github();
    let mut review = pull.open();
    review.pull_viewer_authorship(&github).unwrap();

    review.finish_review(ReviewVerdict::Approve, "ship it", &github);
    review.pull_last_review(&github).unwrap();

    let last = review.last_review().unwrap();
    assert_eq!(last.outcome, ReviewOutcome::Approved);
    assert_eq!(last.body, "ship it");
    assert_eq!(last.commit, Sha::new(&pull.head));
}

#[test]
fn a_last_review_that_could_not_be_read_is_a_problem_until_it_is_read() {
    let pull = Pull::new();
    let github = pull.github();
    github.inject(GitHubFault::ReadsFail);
    let mut review = pull.open();

    review.pull_last_review(&github).unwrap_err();
    assert!(
        review
            .problems()
            .iter()
            .any(|problem| problem.to_string().contains("last review"))
    );

    github.heal(&GitHubFault::ReadsFail);
    review.pull_last_review(&github).unwrap();
    assert_eq!(review.problems().len(), 0);
}
