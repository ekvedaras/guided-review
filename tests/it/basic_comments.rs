use crate::support::app_path::AppPath;
use crate::support::fake_ports::{FakeHost, HostCall, HostFault, SentLog};
use crate::support::{FixtureRepo, drafts, file_path, line, save_typing};
use guided_review::local_git::LocalGit;
use guided_review::pull_request::Sha;
use guided_review::review_subject::ReviewSubject;
use guided_review::tour::{BasicTour, TourPin};
use guided_review::viewer::{
    CommentGesture, CommentId, CommentPlace, CommentResult, FollowedLines, LineLocation, PageRow,
    Review, ReviewStateFile, ReviewThread, Screen, SendMode, Side, TextBox, ThreadComment,
    ThreadId, ThreadView,
};
use std::collections::BTreeMap;

const PR: &str = "https://github.com/acme/shop/pull/7";

struct Shop {
    repo: FixtureRepo,
    base: String,
    head: String,
    sent: SentLog,
}

impl Shop {
    fn host(&self) -> FakeHost {
        FakeHost::with_pull("main-tip", &self.head, &self.sent)
    }

    fn new() -> Self {
        let repo = FixtureRepo::cloned_from("acme/shop");
        repo.write("a.txt", "one\ntwo\nthree\n");
        repo.write("b.txt", "uno\ndos\ntres\n");
        let base = repo.commit("base");
        repo.write("a.txt", "one\nTWO\nthree\n");
        repo.write("b.txt", "uno\nDOS\ntres\n");
        let head = repo.commit("head");
        Self {
            repo,
            base,
            head,
            sent: SentLog::default(),
        }
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
            "Orders cost two".to_owned(),
        );
        let source = LocalGit::find(&crate::support::shop_on_github(), self.repo.path())
            .unwrap()
            .unwrap();
        let state_file = ReviewStateFile::for_subject(
            &self.repo.folders(),
            &PR.parse::<ReviewSubject>().unwrap(),
        );
        Review::open_basic(
            tour,
            &source,
            state_file,
            crate::support::fake_ports::every_capability(),
        )
        .unwrap()
    }
}

fn comment(body: &str) -> ThreadComment {
    ThreadComment {
        avatar_url: None,
        id: CommentId::new(&format!("C_{body}")),
        reactions: Vec::new(),
        author: "alice".into(),
        is_bot: false,
        by_agent: None,
        by_viewer: false,
        body: body.into(),
        url: format!("https://github.com/acme/shop/pull/7#discussion_{body}"),
    }
}

fn thread(id: &str, path: &str, side: Side, number: usize) -> ReviewThread {
    ReviewThread {
        id: ThreadId::new(id),
        is_resolved: false,
        is_outdated: false,
        path: file_path(path),
        line: Some(line(number)),
        start_line: None,
        side,
        is_file_comment: false,
        original_line: Some(line(number)),
        original_start_line: None,
        original_commit: Some("abc".into()),
        comments: vec![comment("why?")],
    }
}

fn ids(threads: &[ThreadView]) -> Vec<&str> {
    threads.iter().map(|view| view.thread.id.as_str()).collect()
}

fn on_file_screen(review: &mut Review, path: &str) {
    while review.go_back() {}
    let target = Screen::File(file_path(path));
    while review.screen() != &target {
        assert!(review.go_forward(), "{path} is not in the reading order");
    }
}

#[test]
fn a_thread_hangs_under_its_line_on_the_screen_of_its_file() {
    let shop = Shop::new();
    let mut review = shop.open();
    review.set_threads(&[
        thread("t1", "a.txt", Side::Head, 2),
        thread("t2", "b.txt", Side::Base, 2),
    ]);

    on_file_screen(&mut review, "a.txt");
    let page = review.file_page().unwrap();
    assert_eq!(ids(&page.threads), ["t1"]);
    let anchor = page.threads[0].anchor.as_ref().unwrap();
    assert_eq!((anchor.side, anchor.line), (Side::Head, line(2)));

    on_file_screen(&mut review, "b.txt");
    let page = review.file_page().unwrap();
    assert_eq!(ids(&page.threads), ["t2"]);
    assert_eq!(page.threads[0].anchor.as_ref().unwrap().side, Side::Base);
}

#[test]
fn a_thread_on_a_file_comment_shows_after_the_code_of_its_file() {
    let shop = Shop::new();
    let mut review = shop.open();
    let on_file = ReviewThread {
        is_file_comment: true,
        line: None,
        original_line: None,
        ..thread("f1", "a.txt", Side::Head, 1)
    };
    review.set_threads(&[on_file]);

    on_file_screen(&mut review, "a.txt");
    let page = review.file_page().unwrap();
    assert_eq!(ids(&page.threads), ["f1"]);
    assert!(page.threads[0].anchor.is_none());
}

#[test]
fn an_outdated_thread_hangs_under_the_lines_it_was_followed_to() {
    let shop = Shop::new();
    let mut review = shop.open();
    let outdated = ReviewThread {
        is_outdated: true,
        line: None,
        ..thread("old", "a.txt", Side::Head, 9)
    };
    review.set_threads(std::slice::from_ref(&outdated));
    on_file_screen(&mut review, "a.txt");
    assert!(review.file_page().unwrap().threads[0].anchor.is_none());

    review.set_followed_lines(BTreeMap::from([(
        ThreadId::new("old"),
        FollowedLines {
            start: line(2),
            end: line(2),
            before: vec!["old two".to_owned()],
            were_changed: true,
        },
    )]));

    let page = review.file_page().unwrap();
    assert_eq!(ids(&page.threads), ["old"]);
    let anchor = page.threads[0].anchor.as_ref().unwrap();
    assert_eq!((anchor.side, anchor.line), (Side::Head, line(2)));
}

#[test]
fn the_overview_counts_the_open_threads_of_every_file() {
    let shop = Shop::new();
    let mut review = shop.open();
    review.set_threads(&[
        thread("t1", "a.txt", Side::Head, 2),
        thread("t2", "a.txt", Side::Head, 1),
        thread("t3", "b.txt", Side::Head, 2),
    ]);

    assert_eq!(review.overview_page().stats.open_threads, 3);
}

fn row(review: &Review, text: &str) -> LineLocation {
    review
        .file_page()
        .unwrap()
        .pieces
        .iter()
        .flat_map(guided_review::viewer::PieceDiff::lines)
        .find(|line| line.text == text)
        .unwrap_or_else(|| panic!("no row shows {text:?}"))
        .location
        .clone()
}

fn comment_on_line(review: &mut Review, text: &str) -> CommentPlace {
    let at = row(review, text);
    review.begin_selection(at.clone());
    review.extend_selection(at);
    review
        .open_comment_box(CommentGesture::DragEnded)
        .unwrap()
        .unwrap()
}

fn typed(review: &mut Review, place: &CommentPlace, body: &str) {
    review
        .type_in(&TextBox::Draft(place.clone()), body)
        .unwrap();
    save_typing(review);
}

#[test]
fn a_line_comment_is_sent_from_a_file_screen_into_the_pending_review() {
    let shop = Shop::new();
    let host = shop.host();
    let mut review = shop.open();
    on_file_screen(&mut review, "a.txt");
    let place = comment_on_line(&mut review, "TWO");
    typed(&mut review, &place, "why caps?");

    let result = review.send_comment(&place, SendMode::AddToReview, &host);

    assert_eq!(result.unwrap(), Some(CommentResult::Sent));
    assert_eq!(
        shop.sent.host_calls().iter().rev().nth(1).unwrap(),
        &HostCall::AddThread {
            pr: PR.parse().unwrap(),
            place,
            body: "why caps?".into()
        }
    );
    assert_eq!(drafts(&review), []);
}

#[test]
fn a_file_comment_is_sent_from_a_file_screen_and_its_draft_survives_a_restart() {
    let shop = Shop::new();
    let host = shop.host();
    let mut review = shop.open();
    on_file_screen(&mut review, "b.txt");
    let path = file_path("b.txt");
    let place = review
        .open_comment_box(CommentGesture::OnFile(&path))
        .unwrap()
        .unwrap();
    typed(&mut review, &place, "whole file");

    let reopened = shop.open();
    assert_eq!(drafts(&reopened).len(), 1);
    assert_eq!(drafts(&reopened)[0].body, "whole file");

    let result = review.send_comment(&place, SendMode::AddToReview, &host);
    assert_eq!(result.unwrap(), Some(CommentResult::Sent));
    assert!(matches!(
        shop.sent.host_calls().iter().rev().nth(1).unwrap(),
        HostCall::AddThread { place: sent, .. } if sent == &CommentPlace::on_file(&path)
    ));
}

#[test]
fn a_file_screen_offers_a_comment_on_its_file_and_the_open_box_shows_under_the_toolbar() {
    let shop = Shop::new();
    let mut review = shop.open();
    on_file_screen(&mut review, "b.txt");
    let path = file_path("b.txt");
    let offered = review.page().rows().iter().find_map(|row| match row {
        PageRow::Toolbar(toolbar) => toolbar.comment_on_file.clone(),
        _ => None,
    });
    assert_eq!(offered, Some(path.clone()));

    let place = review
        .open_comment_box(CommentGesture::OnFile(&path))
        .unwrap()
        .unwrap();

    let kinds: Vec<&str> = review
        .page()
        .rows()
        .iter()
        .filter_map(|row| match row {
            PageRow::Toolbar(_) => Some("toolbar"),
            PageRow::Composer { place: at, .. } if **at == place => Some("composer"),
            PageRow::Line(_) => Some("line"),
            _ => None,
        })
        .collect();
    assert_eq!(&kinds[..3], ["toolbar", "composer", "line"]);
    assert_eq!(kinds.iter().filter(|kind| **kind == "composer").count(), 1);
}

#[test]
fn the_overview_offers_no_comment_on_a_file() {
    let shop = Shop::new();
    let review = shop.open();

    assert!(review.page().rows().iter().all(|row| match row {
        PageRow::Toolbar(toolbar) => toolbar.comment_on_file.is_none(),
        _ => true,
    }));
}

#[test]
fn a_queued_comment_of_a_basic_review_survives_a_restart() {
    let shop = Shop::new();
    let host = shop.host();
    let mut review = shop.open();
    on_file_screen(&mut review, "a.txt");
    let place = comment_on_line(&mut review, "TWO");
    typed(&mut review, &place, "later");
    host.inject(HostFault::ConnectionLost);

    let result = review.send_comment(&place, SendMode::AddToReview, &host);

    assert!(matches!(result.unwrap(), Some(CommentResult::Queued(_))));
    assert_eq!(shop.open().queued_comments().len(), 1);
}
