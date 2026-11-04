#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "a panic in a test helper is the test failing"
)]

#[allow(dead_code)]
mod support;

use guided_review::local_git::LocalGit;
use guided_review::pull_request::{PullRequestUrl, RepoName};
use guided_review::tour::Tour;
use guided_review::viewer::{
    CommentPlace, CommentRefusal, CommentResult, LineLocation, Review, ReviewStateFile, SendMode,
    Side,
};
use serde_json::json;
use support::FixtureRepo;
use support::app_path::AppPath;
use support::fake_ports::{FakeGitHub, GitHubCall, GitHubFault, SentLog};

const PR: &str = "https://github.com/acme/shop/pull/7";

fn numbered(change: impl Fn(usize) -> Option<&'static str>) -> String {
    (1..=40)
        .map(|number| match change(number) {
            Some(text) => format!("{text}\n"),
            None => format!("line {number}\n"),
        })
        .collect()
}

/// `notes.txt` changes on line 10 and on line 30: two hunks, a gap between them.
struct Pull {
    repo: FixtureRepo,
    base: String,
    head: String,
    sent: SentLog,
}

impl Pull {
    fn new() -> Self {
        let repo = FixtureRepo::cloned_from("acme/shop");
        repo.write("notes.txt", &numbered(|_| None));
        let base = repo.commit("base");
        repo.write(
            "notes.txt",
            &numbered(|number| match number {
                10 => Some("line ten"),
                30 => Some("line thirty"),
                _ => None,
            }),
        );
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

    fn github_at(&self, head: &str) -> FakeGitHub {
        FakeGitHub::with_pull("main-tip", head, &self.sent)
    }

    fn pr(&self) -> PullRequestUrl {
        PR.parse().unwrap()
    }

    fn open(&self) -> Review {
        self.open_at(&self.head)
    }

    fn open_at(&self, head: &str) -> Review {
        let tour: Tour = serde_json::from_value(json!({
            "pr": PR,
            "base": self.base,
            "head": head,
            "title": "Notes",
            "summary": [],
            "steps": [{"id": "notes", "title": "Notes", "why": "because", "pieces": [{"file": "notes.txt"}], "children": []}],
        }))
        .unwrap();
        let source = LocalGit::find(&RepoName::new("acme", "shop"), self.repo.path())
            .unwrap()
            .unwrap();
        let state_file = ReviewStateFile::for_pr(self.repo.home(), &self.pr());
        let mut review = Review::open(tour, &source, state_file).unwrap();
        let files_at_head = review.files_at_head_request().read(&source);
        review.set_files_at_head(files_at_head);
        review.go_forward();
        review
    }
}

fn row(review: &Review, text: &str) -> LineLocation {
    review
        .current_step()
        .unwrap()
        .pieces
        .iter()
        .flat_map(|piece| piece.lines())
        .find(|line| line.text == text)
        .unwrap_or_else(|| panic!("no row shows {text:?}"))
        .location
        .clone()
}

fn drag(review: &mut Review, from: &str, to: &str) -> Result<Option<CommentPlace>, CommentRefusal> {
    let (from, to) = (row(review, from), row(review, to));
    review.begin_selection(from);
    review.extend_selection(to);
    review.finish_selection()
}

fn comment_on(review: &mut Review, text: &str) -> CommentPlace {
    drag(review, text, text).unwrap().unwrap()
}

fn typed(review: &mut Review, place: &CommentPlace, body: &str) {
    review.edit_draft(place, body).unwrap();
}

#[test]
fn a_click_on_one_row_opens_a_comment_box_on_that_line() {
    let pull = Pull::new();
    let mut review = pull.open();

    let place = comment_on(&mut review, "line ten");

    assert!(place.is_single_line());
    assert_eq!((place.end.line, place.path()), (10, "notes.txt"));
    assert_eq!(review.comment_drafts().len(), 1);
}

#[test]
fn a_drag_selects_every_row_between_across_hunks_and_sides() {
    let pull = Pull::new();
    let mut review = pull.open();
    let (from, to) = (row(&review, "line 10"), row(&review, "line thirty"));

    review.begin_selection(from.clone());
    review.extend_selection(to.clone());
    let selected = review.selected_lines().unwrap();

    assert_eq!(selected.first(), &from);
    assert_eq!(selected.last(), &to);
    let texts: Vec<_> = selected.lines().iter().map(|l| l.text.as_str()).collect();
    assert_eq!(texts[..3], ["line 10", "line ten", "line 11"]);
    assert_eq!(texts.last(), Some(&"line thirty"));
    assert!(
        texts.contains(&"line 28"),
        "rows of the later hunk are in it too"
    );
}

#[test]
fn dragging_upwards_selects_the_same_rows() {
    let pull = Pull::new();
    let mut review = pull.open();
    let place = drag(&mut review, "line thirty", "line 10")
        .unwrap()
        .unwrap();

    assert_eq!((place.start.line, place.end.line), (10, 30));
}

#[test]
fn a_selection_on_expanded_context_takes_no_comment() {
    let pull = Pull::new();
    let mut review = pull.open();
    let more = review.current_step().unwrap().pieces[0].hunks[0]
        .more_above
        .clone()
        .unwrap();
    review.show_more_context(&more);

    let refused = drag(&mut review, "line 5", "line ten");

    assert_eq!(refused, Err(CommentRefusal::NotInsideHunk));
    assert!(review.comment_drafts().is_empty());
}

#[test]
fn unsent_text_survives_a_restart() {
    let pull = Pull::new();
    let mut review = pull.open();
    let place = comment_on(&mut review, "line ten");
    typed(&mut review, &place, "why ten?");

    let reopened = pull.open();

    assert_eq!(reopened.comment_drafts().len(), 1);
    assert_eq!(reopened.comment_drafts()[0].body, "why ten?");
    assert_eq!(reopened.comment_drafts()[0].place, place);
}

#[test]
fn a_comment_goes_into_the_pending_review_by_default() {
    let pull = Pull::new();
    let github = pull.github();
    let mut review = pull.open();
    let place = drag(&mut review, "line ten", "line 12").unwrap().unwrap();
    typed(&mut review, &place, "why?");

    let result = review.send_comment(&place, SendMode::AddToReview, &github);

    assert_eq!(result.unwrap(), Some(CommentResult::Sent));
    assert_eq!(
        pull.sent.github_calls().last().unwrap(),
        &GitHubCall::AddThread {
            pr: PR.parse().unwrap(),
            place,
            body: "why?".into()
        }
    );
    assert!(review.comment_drafts().is_empty(), "a sent draft is gone");
    assert!(github.has_pending_review_now());
}

#[test]
fn a_range_from_a_removed_line_to_an_added_one_goes_left_to_right() {
    let pull = Pull::new();
    let github = pull.github();
    let mut review = pull.open();
    let place = drag(&mut review, "line 10", "line ten").unwrap().unwrap();
    typed(&mut review, &place, "renamed");

    review
        .send_comment(&place, SendMode::AddToReview, &github)
        .unwrap();

    assert_eq!(
        pull.sent.github_calls().last().unwrap(),
        &GitHubCall::AddThread {
            pr: PR.parse().unwrap(),
            place: place.clone(),
            body: "renamed".into()
        }
    );
    assert_eq!(
        (
            place.start.side,
            place.start.line,
            place.end.side,
            place.end.line
        ),
        (Side::Base, 10, Side::Head, 10)
    );
}

#[test]
fn a_pending_review_made_on_the_web_takes_the_comment_and_hides_comment_now() {
    let pull = Pull::new();
    let github = pull.github().with_web_pending_review();
    let mut review = pull.open();
    review.pull_pending_review(&github).unwrap();
    let place = comment_on(&mut review, "line ten");
    typed(&mut review, &place, "hm");

    assert!(!review.can_comment_now());
    let result = review.send_comment(&place, SendMode::AddToReview, &github);

    assert_eq!(result.unwrap(), Some(CommentResult::Sent));
}

#[test]
fn comment_now_shows_only_when_surely_no_pending_review_exists() {
    let pull = Pull::new();
    let github = pull.github();
    let mut review = pull.open();
    assert!(!review.can_comment_now(), "unknown yet");

    review.pull_pending_review(&github).unwrap();

    assert!(review.can_comment_now());
}

#[test]
fn comment_now_posts_at_once_and_leaves_no_pending_review() {
    let pull = Pull::new();
    let github = pull.github();
    let mut review = pull.open();
    review.pull_pending_review(&github).unwrap();
    let place = comment_on(&mut review, "line ten");
    typed(&mut review, &place, "now");

    let result = review.send_comment(&place, SendMode::CommentNow, &github);

    assert_eq!(result.unwrap(), Some(CommentResult::Sent));
    assert!(pull.sent.github_calls().contains(&GitHubCall::CommentNow {
        pr: PR.parse().unwrap(),
        place,
        body: "now".into()
    }));
    assert!(review.can_comment_now());
}

#[test]
fn comment_now_is_refused_when_a_pending_review_appeared_meanwhile_and_the_text_stays() {
    let pull = Pull::new();
    let github = pull.github();
    let mut review = pull.open();
    review.pull_pending_review(&github).unwrap();
    let place = comment_on(&mut review, "line ten");
    typed(&mut review, &place, "now");
    github.inject(GitHubFault::PendingReviewAppearsOnCommentNow);

    let result = review.send_comment(&place, SendMode::CommentNow, &github);

    assert_eq!(
        result.unwrap(),
        Some(CommentResult::Refused(CommentRefusal::PendingReviewExists))
    );
    assert!(!review.can_comment_now());
    assert_eq!(review.comment_drafts()[0].body, "now");
    assert!(review.queued_comments().is_empty());
}

#[test]
fn thread_null_is_a_failure_that_keeps_the_text_and_is_not_retried() {
    let pull = Pull::new();
    let github = pull.github();
    github.inject(GitHubFault::NewThreadsAnswerNull);
    let mut review = pull.open();
    let place = comment_on(&mut review, "line ten");
    typed(&mut review, &place, "lost?");

    let result = review.send_comment(&place, SendMode::AddToReview, &github);

    assert!(matches!(result.unwrap(), Some(CommentResult::Rejected(_))));
    assert_eq!(review.comment_drafts()[0].body, "lost?");
    assert!(review.queued_comments().is_empty());
    assert!(
        !review.can_comment_now(),
        "it left an empty pending review behind"
    );
}

#[test]
fn a_comment_that_cannot_reach_github_waits_in_the_queue_and_goes_later() {
    let pull = Pull::new();
    let github = pull.github();
    let mut review = pull.open();
    let place = comment_on(&mut review, "line ten");
    typed(&mut review, &place, "later");
    github.inject(GitHubFault::ConnectionLost);

    let result = review.send_comment(&place, SendMode::AddToReview, &github);

    assert!(matches!(result.unwrap(), Some(CommentResult::Queued(_))));
    assert!(review.comment_drafts().is_empty());
    assert_eq!(review.queued_comments().len(), 1);
    assert_eq!(
        pull.open().queued_comments().len(),
        1,
        "it is in the state file"
    );

    github.heal(&GitHubFault::ConnectionLost);
    review.send_queued_comments(&github).unwrap();

    assert!(review.queued_comments().is_empty());
    assert!(github.has_pending_review_now());
}

#[test]
fn a_queued_comment_keeps_waiting_while_github_is_unreachable() {
    let pull = Pull::new();
    let github = pull.github();
    let mut review = pull.open();
    let place = comment_on(&mut review, "line ten");
    typed(&mut review, &place, "later");
    github.inject(GitHubFault::ConnectionLost);
    review
        .send_comment(&place, SendMode::AddToReview, &github)
        .unwrap();

    review.send_queued_comments(&github).unwrap();

    assert_eq!(review.queued_comments().len(), 1);
}

#[test]
fn nothing_is_written_while_the_tour_is_behind_the_pr_head() {
    let pull = Pull::new();
    let github = pull.github_at("newer-head");
    let mut review = pull.open();
    let place = comment_on(&mut review, "line ten");
    typed(&mut review, &place, "stale");

    let result = review.send_comment(&place, SendMode::AddToReview, &github);

    assert_eq!(
        result.unwrap(),
        Some(CommentResult::Refused(CommentRefusal::TourIsOld))
    );
    assert_eq!(
        CommentRefusal::TourIsOld.to_string(),
        "Update tour to comment"
    );
    assert!(
        pull.sent
            .github_calls()
            .iter()
            .all(|call| !matches!(call, GitHubCall::AddThread { .. })),
        "{:?}",
        pull.sent.github_calls()
    );
    assert_eq!(review.comment_drafts()[0].body, "stale");
}

#[test]
fn a_new_comment_box_cannot_open_while_the_tour_is_behind_the_pr_head() {
    let pull = Pull::new();
    let mut review = pull.open();
    review.check_pr_head(&pull.github_at("newer-head")).unwrap();

    let refused = drag(&mut review, "line ten", "line ten");

    assert_eq!(refused, Err(CommentRefusal::TourIsOld));
}

#[test]
fn discarding_removes_the_draft_from_the_state_file() {
    let pull = Pull::new();
    let mut review = pull.open();
    let place = comment_on(&mut review, "line ten");

    review.discard_draft(&place).unwrap();

    assert!(pull.open().comment_drafts().is_empty());
}

#[test]
fn a_draft_written_before_a_repin_keeps_its_text_but_is_not_sent_to_the_old_lines() {
    let pull = Pull::new();
    let mut review = pull.open();
    let place = comment_on(&mut review, "line ten");
    typed(&mut review, &place, "why ten?");
    pull.repo.write("other.txt", "new\n");
    let later = pull.repo.commit("later");
    let mut repinned = pull.open_at(&later);

    let result = repinned.send_comment(&place, SendMode::AddToReview, &pull.github_at(&later));

    assert_eq!(
        result.unwrap(),
        Some(CommentResult::Refused(CommentRefusal::CodeMoved))
    );
    assert_eq!(
        CommentRefusal::CodeMoved.to_string(),
        "Code moved, place the comment again"
    );
    assert_eq!(repinned.comment_drafts()[0].body, "why ten?");
    assert!(
        pull.sent
            .github_calls()
            .iter()
            .all(|call| !matches!(call, GitHubCall::AddThread { .. }))
    );
}

#[test]
fn a_queued_comment_from_before_a_repin_is_not_sent_to_the_old_lines() {
    let pull = Pull::new();
    let mut review = pull.open();
    let place = comment_on(&mut review, "line ten");
    typed(&mut review, &place, "later please");
    let offline = pull.github();
    offline.inject(GitHubFault::ConnectionLost);
    review
        .send_comment(&place, SendMode::AddToReview, &offline)
        .unwrap();
    assert_eq!(review.queued_comments().len(), 1);
    pull.repo.write("other.txt", "new\n");
    let later = pull.repo.commit("later");
    let mut repinned = pull.open_at(&later);

    let before = pull.sent.github_calls().len();
    repinned
        .send_queued_comments(&pull.github_at(&later))
        .unwrap();

    assert_eq!(repinned.queued_comments().len(), 1);
    assert_eq!(repinned.queued_comments()[0].draft.body, "later please");
    assert_eq!(pull.sent.github_calls().len(), before);
}
