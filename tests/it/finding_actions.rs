//! What the reviewer does with one finding: use it, use it as is, dismiss it.

use crate::support::app_path::AppPath;
use crate::support::fake_ports::GitHubFault;
use crate::support::fake_ports::{FakeGitHub, GitHubCall, SentLog};
use crate::support::{FixtureRepo, file_path, line};
use guided_review::findings::FindingId;
use guided_review::local_work_tree::LocalWorkTree;
use guided_review::open_review::open_review;
use guided_review::review_brief::ReviewBrief;
use guided_review::review_subject::ReviewSubject;
use guided_review::tour::Tour;
use guided_review::viewer::{
    CommentDraft, CommentPlace, CommentRefusal, CommentResult, FindingRefusal, LineLocation, Page,
    PageRow, Review, SendMode, Side,
};
use serde_json::{Value, json};

const PR: &str = "https://github.com/acme/shop/pull/7";

fn pr() -> ReviewSubject {
    PR.parse().unwrap()
}

/// `README.md` changes on line 2 and is the only step.
struct Shop {
    repo: FixtureRepo,
    head: String,
}

impl Shop {
    fn with_findings(findings: &[Value]) -> Self {
        let repo = FixtureRepo::cloned_from("acme/shop");
        repo.write("README.md", "hello\nbye\nend\n");
        let base = repo.commit("base");
        repo.write("README.md", "hello\nworld\nend\n");
        let head = repo.commit("head");
        let tour = json!({
            "pr": PR, "base": base, "head": head, "title": "Readme", "summary": [],
            "steps": [{"id": "readme", "title": "Readme", "why": "w", "pieces": [{"file": "README.md"}]}],
        });
        let path = Tour::path_for(&repo.folders(), &pr());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, tour.to_string()).unwrap();
        std::fs::write(
            Tour::findings_path_for(&repo.folders(), &pr()),
            json!({"pr": PR, "base": base, "head": head, "findings": findings}).to_string(),
        )
        .unwrap();
        Self { repo, head }
    }

    /// The review as the app opens it, on its only step.
    fn open(&self) -> Review {
        let mut review = open_review(&pr(), self.repo.path(), &self.repo.folders())
            .unwrap()
            .review;
        review.go_forward();
        review
    }
}

fn finding(id: &str, severity: &str, body: &str, anchor: Value) -> Value {
    json!({"id": id, "severity": severity, "body": body, "anchor": anchor})
}

fn on_line_2(id: &str, body: &str) -> Value {
    finding(
        id,
        "should fix",
        body,
        json!({"kind": "lines", "file": "README.md", "side": "head", "lines": [2, 2]}),
    )
}

fn line_2() -> LineLocation {
    LineLocation {
        path: file_path("README.md"),
        side: Side::Head,
        line: line(2),
    }
}

fn id(text: &str) -> FindingId {
    FindingId::try_from(text.to_owned()).unwrap()
}

fn drafts(review: &Review) -> Vec<CommentDraft> {
    crate::support::drafts(review)
}

fn shown_findings(page: &Page) -> Vec<String> {
    page.rows()
        .iter()
        .filter_map(|row| match row {
            PageRow::Finding(finding) => Some(finding.id.to_string()),
            _ => None,
        })
        .collect()
}

#[test]
fn a_dismissed_finding_goes_from_its_screen_and_stays_gone_when_the_review_opens_again() {
    let shop = Shop::with_findings(&[on_line_2("a", "One."), on_line_2("b", "Two.")]);
    let mut review = shop.open();

    review.dismiss_finding(&id("a")).unwrap();

    assert_eq!(shown_findings(review.page()), ["b"]);
    assert_eq!(review.ai_review().counts().total(), 1);
    assert_eq!(shown_findings(shop.open().page()), ["b"]);
}

#[test]
fn using_a_finding_puts_its_text_in_a_comment_box_on_its_lines_and_the_finding_goes_when_it_is_sent()
 {
    let shop = Shop::with_findings(&[on_line_2("a", "Say it twice."), on_line_2("b", "Two.")]);
    let mut review = shop.open();
    let sent = SentLog::default();
    let github = FakeGitHub::with_pull("main-tip", &shop.head, &sent);

    let place = review.use_finding(&id("a")).unwrap();

    let CommentPlace::Lines { start, end } = &place else {
        panic!("a finding on lines is a comment on lines");
    };
    assert_eq!((start.line.get(), end.line.get()), (2, 2));
    assert_eq!(review.composing(), Some(&place));
    assert_eq!(drafts(&review)[0].body, "Say it twice.");
    assert_eq!(shown_findings(review.page()), ["a", "b"]);
    assert_eq!(drafts(&shop.open())[0].body, "Say it twice.");
    assert_eq!(shown_findings(shop.open().page()), ["a", "b"]);

    let result = review.send_comment(&place, SendMode::AddToReview, &github);

    assert_eq!(result.unwrap(), Some(CommentResult::Sent));
    assert_eq!(shown_findings(review.page()), ["b"]);
    assert_eq!(shown_findings(shop.open().page()), ["b"]);
}

#[test]
fn a_finding_used_in_a_box_whose_send_is_rejected_stays_open() {
    let shop = Shop::with_findings(&[on_line_2("a", "Say it twice.")]);
    let mut review = shop.open();
    let sent = SentLog::default();
    let github = FakeGitHub::with_pull("main-tip", &shop.head, &sent);
    github.inject(GitHubFault::NewThreadsAnswerNull);
    let place = review.use_finding(&id("a")).unwrap();

    let result = review.send_comment(&place, SendMode::AddToReview, &github);

    assert!(matches!(result.unwrap(), Some(CommentResult::Rejected(_))));
    assert_eq!(shown_findings(review.page()), ["a"]);
}

#[test]
fn using_a_finding_as_is_whose_send_is_rejected_keeps_the_finding() {
    let shop = Shop::with_findings(&[on_line_2("a", "Say it twice.")]);
    let mut review = shop.open();
    let sent = SentLog::default();
    let github = FakeGitHub::with_pull("main-tip", &shop.head, &sent);
    github.inject(GitHubFault::NewThreadsAnswerNull);

    let request = review
        .use_finding_as_is(&id("a"), SendMode::AddToReview)
        .unwrap();
    let result = review.apply_comment(request.run(&github)).unwrap();

    assert!(matches!(result, CommentResult::Rejected(_)));
    assert_eq!(shown_findings(review.page()), ["a"]);
}

#[test]
fn using_a_finding_on_a_file_comments_on_the_file_and_one_about_the_whole_change_has_no_lines() {
    let shop = Shop::with_findings(&[
        finding(
            "f",
            "nit",
            "File.",
            json!({"kind": "file", "file": "README.md"}),
        ),
        finding("c", "nit", "Change.", json!({"kind": "change"})),
    ]);
    let mut review = shop.open();

    let on_file = review.use_finding(&id("f")).unwrap();
    let on_change = review.use_finding(&id("c"));

    assert!(on_file.is_file_comment());
    assert_eq!(
        on_change,
        Err(FindingRefusal::Comment(CommentRefusal::NoLines))
    );
    assert_eq!(review.ai_review().counts().total(), 2);
}

#[test]
fn a_finding_that_is_not_open_cannot_be_used_or_dismissed() {
    let shop = Shop::with_findings(&[on_line_2("a", "One.")]);
    let mut review = shop.open();
    review.dismiss_finding(&id("a")).unwrap();

    assert_eq!(review.use_finding(&id("a")), Err(FindingRefusal::NotThere));
    assert_eq!(
        review.dismiss_finding(&id("a")),
        Err(FindingRefusal::NotThere)
    );
}

#[test]
fn using_a_finding_as_is_adds_it_to_the_review_marked_as_ai_without_a_box_to_edit() {
    let shop = Shop::with_findings(&[on_line_2("a", "Say it twice."), on_line_2("b", "Two.")]);
    let mut review = shop.open();
    let sent = SentLog::default();

    let request = review
        .use_finding_as_is(&id("a"), SendMode::AddToReview)
        .unwrap();
    let result = review
        .apply_comment(request.run(&FakeGitHub::with_pull("main-tip", &shop.head, &sent)))
        .unwrap();

    assert_eq!(result, CommentResult::Sent);
    let added: Vec<String> = sent
        .github_calls()
        .into_iter()
        .filter_map(|call| match call {
            GitHubCall::AddThread { body, .. } => Some(body),
            _ => None,
        })
        .collect();
    assert_eq!(added, ["🤖 Say it twice."]);
    assert_eq!(shown_findings(review.page()), ["b"]);
}

#[test]
fn using_a_finding_as_is_with_a_send_action_the_review_does_not_offer_leaves_it_open() {
    let shop = Shop::with_findings(&[on_line_2("a", "One.")]);
    let mut review = shop.open();

    let refused = review.use_finding_as_is(&id("a"), SendMode::AddToTasks);

    assert!(matches!(
        refused,
        Err(FindingRefusal::Comment(CommentRefusal::NotOffered))
    ));
    assert_eq!(shown_findings(review.page()), ["a"]);
    assert_eq!(drafts(&review).len(), 0);
}

#[test]
fn using_a_finding_as_is_does_not_send_a_comment_the_reviewer_is_writing_on_those_lines() {
    let shop = Shop::with_findings(&[on_line_2("a", "One."), on_line_2("b", "Two.")]);
    let mut review = shop.open();
    review.use_finding(&id("a")).unwrap();

    let refused = review.use_finding_as_is(&id("b"), SendMode::AddToReview);

    assert!(matches!(refused, Err(FindingRefusal::DraftInTheWay(_))));
    assert_eq!(shown_findings(review.page()), ["a", "b"]);
    assert_eq!(drafts(&review)[0].body, "One.");
}

/// The findings and comment boxes of a page, in order: `finding a`, `box`.
fn findings_and_boxes(page: &Page) -> Vec<String> {
    page.rows()
        .iter()
        .filter_map(|row| match row {
            PageRow::Finding(finding) => Some(format!("finding {}", finding.id)),
            PageRow::Composer { .. } => Some("box".to_owned()),
            _ => None,
        })
        .collect()
}

#[test]
fn using_a_finding_on_a_whole_file_shows_its_comment_box_under_the_finding() {
    let shop = Shop::with_findings(&[
        finding(
            "a",
            "nit",
            "Split it.",
            json!({"kind": "file", "file": "README.md"}),
        ),
        on_line_2("b", "Two."),
    ]);
    let mut review = shop.open();

    review.use_finding(&id("a")).unwrap();

    assert_eq!(
        findings_and_boxes(review.page()),
        ["finding b", "finding a", "box"]
    );
}

#[test]
fn using_a_finding_in_a_folded_box_shows_its_comment_box_under_the_finding() {
    let shop = Shop::with_findings(&[on_line_2("a", "Say it twice.")]);
    let mut review = shop.open();
    let screen = review.screen().clone();
    review.toggle_box_fold(&screen, &file_path("README.md"));
    assert!(review.page().row_of_line(&line_2()).is_none());

    review.use_finding(&id("a")).unwrap();

    assert_eq!(findings_and_boxes(review.page()), ["finding a", "box"]);
}

#[test]
fn using_a_finding_as_is_with_a_comment_box_on_its_lines_names_that_box() {
    let shop = Shop::with_findings(&[on_line_2("a", "One."), on_line_2("b", "Two.")]);
    let mut review = shop.open();
    let place = review.use_finding(&id("a")).unwrap();

    let refused = review.use_finding_as_is(&id("b"), SendMode::AddToReview);

    assert_eq!(
        refused.err(),
        Some(FindingRefusal::DraftInTheWay(place.clone()))
    );
    assert!(review.page().row_of_comment_box(&place).is_some());
}

#[test]
fn using_a_finding_as_is_in_a_local_review_adds_it_to_the_tasks() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    repo.write("README.md", "hello\n");
    repo.commit("Start the shop");
    repo.write("README.md", "hello\nworld\n");
    let subject = ReviewSubject::from(
        LocalWorkTree::containing(repo.path())
            .unwrap()
            .change_set()
            .unwrap(),
    );
    let pin = json!({"local": subject.to_string(), "findings": [finding(
        "a", "nit", "Say it twice.",
        json!({"kind": "lines", "file": "README.md", "side": "head", "lines": [2, 2]}),
    )]});
    let path = Tour::findings_path_for(&repo.folders(), &subject);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, pin.to_string()).unwrap();
    let mut review = open_review(&subject, repo.path(), &repo.folders())
        .unwrap()
        .review;

    let request = review
        .use_finding_as_is(&id("a"), SendMode::AddToTasks)
        .unwrap();
    review
        .apply_comment(request.run(&FakeGitHub::with_pull(
            "main-tip",
            "snapshot",
            &SentLog::default(),
        )))
        .unwrap();

    assert_eq!(review.queued_tasks(), 1);
    assert_eq!(review.ai_review().counts().total(), 0);
}

#[test]
fn the_brief_of_a_second_run_lists_the_dismissed_findings_and_not_the_used_ones() {
    let shop = Shop::with_findings(&[
        on_line_2("a", "Rename it."),
        on_line_2("b", "Say it twice."),
        on_line_2("c", "Still open."),
    ]);
    let mut review = shop.open();
    review.dismiss_finding(&id("a")).unwrap();
    review.use_finding(&id("b")).unwrap();

    let brief = ReviewBrief::of(&shop.repo.folders(), &pr()).to_string();

    assert!(brief.contains("dismissed findings:"), "{brief}");
    assert!(brief.contains("Rename it."), "{brief}");
    assert!(!brief.contains("dismissed findings: none"), "{brief}");
    assert!(!brief.contains("Say it twice."), "{brief}");
    assert!(!brief.contains("Still open."), "{brief}");
}
