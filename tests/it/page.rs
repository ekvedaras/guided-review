//! The page of the open screen: built once per change of the review and kept until the next one.

use std::sync::Arc;

use crate::support::{FixtureRepo, file_path, line};
use guided_review::local_git::LocalGit;
use guided_review::pull_request::RepoName;
use guided_review::tour::Tour;
use guided_review::viewer::{
    BoxEdges, CommentGesture, CommentId, LineKind, LineLocation, Page, PageRow, Review,
    ReviewStateFile, ReviewThread, Side, ThreadComment, ThreadId, ThreadRowPlace,
};
use serde_json::{Value, json};

const PR: &str = "https://github.com/acme/shop/pull/7";

const CART_AT_BASE: &str = "<?php

final class Cart
{
    public function total(): int
    {
        $a = 1;
        $b = 2;
        $c = 3;
        $d = 4;
        return $a;
    }

    public function count(): int
    {
        return 0;
    }
}
";

fn numbered(change: impl Fn(usize) -> Option<&'static str>) -> String {
    (1..=40)
        .map(|number| match change(number) {
            Some(text) => format!("{text}\n"),
            None => format!("line {number}\n"),
        })
        .collect()
}

/// `notes.txt` changes on lines 10 and 30, `other.txt` on line 2.
struct Pull {
    repo: FixtureRepo,
    base: String,
    head: String,
}

impl Pull {
    fn new() -> Self {
        let repo = FixtureRepo::cloned_from("acme/shop");
        repo.write("notes.txt", &numbered(|_| None));
        repo.write("other.txt", "one\ntwo\nthree\n");
        let base = repo.commit("base");
        repo.write(
            "notes.txt",
            &numbered(|number| match number {
                10 => Some("line ten"),
                30 => Some("line thirty"),
                _ => None,
            }),
        );
        repo.write("other.txt", "one\nTWO\nthree\n");
        let head = repo.commit("head");
        Self { repo, base, head }
    }

    /// `app/Cart.php` gains a property in `Cart` and changes a line of `Cart::total`.
    fn cart() -> Self {
        let repo = FixtureRepo::cloned_from("acme/shop");
        repo.write("app/Cart.php", CART_AT_BASE);
        let base = repo.commit("base");
        repo.write(
            "app/Cart.php",
            &CART_AT_BASE
                .replace(
                    "{\n    public function total",
                    "{\n    private int $items = 0;\n    public function total",
                )
                .replace("$d = 4;", "$d = 5;"),
        );
        let head = repo.commit("head");
        Self { repo, base, head }
    }

    /// Opens the tour on its first step, with the files at head read.
    fn open(&self, steps: Value) -> Review {
        let tour: Tour = serde_json::from_value(json!({
            "pr": PR, "base": self.base, "head": self.head, "title": "Notes", "summary": [],
            "steps": steps,
        }))
        .unwrap();
        let source = LocalGit::find(&RepoName::new("acme", "shop"), self.repo.path())
            .unwrap()
            .unwrap();
        let state_file = ReviewStateFile::for_pr(self.repo.home(), &PR.parse().unwrap());
        let mut review = Review::open(tour, &source, state_file).unwrap();
        let files_at_head = review.files_at_head_request().read(&source);
        review.set_files_at_head(files_at_head);
        review.go_forward();
        review
    }
}

fn step(id: &str, file: &str) -> Value {
    json!({"id": id, "title": id, "why": "because", "pieces": [{"file": file}], "children": []})
}

fn two_steps() -> Value {
    json!([step("notes", "notes.txt"), step("other", "other.txt")])
}

fn at_head(path: &str, number: usize) -> LineLocation {
    LineLocation {
        path: file_path(path),
        side: Side::Head,
        line: line(number),
    }
}

#[test]
fn the_page_is_built_once_and_kept_while_the_review_does_not_change() {
    let mut review = Pull::new().open(two_steps());
    review.begin_selection(at_head("notes.txt", 10));
    let page = Arc::clone(review.page());

    let selected = review.selected_lines().unwrap();
    review.place_of_lines(&selected);
    review.image_preview_requests();

    assert!(Arc::ptr_eq(&page, review.page()));
}

#[test]
fn a_change_of_the_review_builds_its_page_again() {
    let mut review = Pull::new().open(two_steps());
    let page = Arc::clone(review.page());

    review.toggle_box_fold(&"notes".into(), &file_path("notes.txt"));

    assert!(!Arc::ptr_eq(&page, review.page()));
}

#[test]
fn a_selection_is_read_off_the_screen_shown_now() {
    let mut review = Pull::new().open(two_steps());
    review.begin_selection(at_head("notes.txt", 10));
    assert!(review.selected_lines().is_some());

    review.go_forward();

    assert!(review.selected_lines().is_none());
}

#[test]
fn lines_shown_by_a_change_of_display_can_be_selected_at_once() {
    let mut review = Pull::new().open(two_steps());
    review.begin_selection(at_head("notes.txt", 16));
    assert!(review.selected_lines().is_none(), "line 16 is hidden");

    let more = review.current_step().unwrap().pieces[0].hunks[0]
        .more_below
        .clone()
        .unwrap();
    review.display_options_mut().show_more_context(&more);

    assert!(review.selected_lines().is_some());
}

/// One short word per row: `+10` for an added line 10, `-10` removed, ` 9` unchanged.
fn shape(row: &PageRow) -> String {
    match row {
        PageRow::StepHeading(_) => "heading".into(),
        PageRow::StepIntro(_) => "intro".into(),
        PageRow::Toolbar(_) => "toolbar".into(),
        PageRow::ScreenFooter(_) => "footer".into(),
        PageRow::PieceHeader(heading) => format!("header {}", heading.piece),
        PageRow::PieceMarker(heading) => format!("marker {}", heading.piece),
        PageRow::Line(line) => {
            let sign = match line.kind {
                LineKind::Removed => '-',
                LineKind::Added => '+',
                LineKind::Context { .. } => ' ',
            };
            format!("{sign}{}", line.location.line)
        }
        PageRow::HunkGap(_) => "gap".into(),
        PageRow::PieceEnd => "end".into(),
        PageRow::Thread { view, place, .. } => format!("thread {} {place:?}", view.thread.id),
        PageRow::Composer { .. } => "comment box".into(),
        PageRow::QueuedComment(_) => "queued".into(),
        PageRow::AskMarker(_) => "ask marker".into(),
        PageRow::Image { .. } => "image".into(),
        PageRow::NoChangeSinceRead => "no change since read".into(),
        _ => "other".into(),
    }
}

fn shapes(page: &Page) -> Vec<String> {
    page.rows().iter().map(shape).collect()
}

/// The rows from the first that reads `from`, `count` of them.
fn rows_from(page: &Page, from: &str, count: usize) -> Vec<String> {
    shapes(page)
        .into_iter()
        .skip_while(|row| row != from)
        .take(count)
        .collect()
}

fn row_of(page: &Page, shown: &str) -> usize {
    shapes(page).iter().position(|row| row == shown).unwrap()
}

fn thread_on(id: &str, line_at: Option<usize>) -> ReviewThread {
    ReviewThread {
        id: ThreadId::new(id),
        is_resolved: false,
        is_outdated: false,
        path: file_path("notes.txt"),
        line: line_at.map(line),
        start_line: None,
        side: Side::Head,
        is_file_comment: line_at.is_none(),
        original_line: line_at.map(line),
        original_start_line: None,
        original_commit: None,
        comments: vec![ThreadComment {
            avatar_url: None,
            id: CommentId::new(&format!("{id}-1")),
            author: "alice".into(),
            is_bot: false,
            by_viewer: false,
            body: "why?".into(),
            url: String::new(),
            reactions: Vec::new(),
        }],
    }
}

fn notes_step() -> Value {
    json!([step("notes", "notes.txt")])
}

#[test]
fn a_page_row_stays_small_since_there_is_one_per_line() {
    assert_eq!(size_of::<PageRow>(), 48);
}

#[test]
fn a_comment_box_hangs_under_the_last_line_of_its_place_and_splits_the_box_around_it() {
    let mut review = Pull::new().open(notes_step());
    review.begin_selection(at_head("notes.txt", 9));
    review.extend_selection(at_head("notes.txt", 10));
    review.open_comment_box(CommentGesture::DragEnded).unwrap();

    let page = review.page();

    assert_eq!(rows_from(page, "+10", 3), ["+10", "comment box", " 11"]);
    let line_ten = row_of(page, "+10");
    assert_eq!(
        page.edges(line_ten),
        BoxEdges {
            opens: false,
            closes: true
        }
    );
    assert_eq!(page.edges(line_ten + 1), BoxEdges::default());
    assert_eq!(
        page.edges(line_ten + 2),
        BoxEdges {
            opens: true,
            closes: false
        }
    );
}

#[test]
fn under_a_line_threads_come_before_comment_boxes_and_each_shows_once() {
    let mut review = Pull::new().open(notes_step());
    review.set_threads(&[thread_on("T1", Some(10))]);
    review.begin_selection(at_head("notes.txt", 10));
    review.open_comment_box(CommentGesture::DragEnded).unwrap();

    let page = review.page();

    assert_eq!(
        rows_from(page, "+10", 3),
        ["+10", "thread T1 UnderLine", "comment box"]
    );
    assert_eq!(
        shapes(page)
            .iter()
            .filter(|row| *row == "comment box")
            .count(),
        1
    );
}

#[test]
fn a_thread_with_no_line_to_hang_under_follows_the_code() {
    let mut review = Pull::new().open(notes_step());
    review.set_threads(&[thread_on("T1", None)]);

    let shapes = shapes(review.page());

    assert_eq!(
        shapes[shapes.len() - 3..],
        ["end", "thread T1 AfterCode", "footer"]
    );
}

#[test]
fn a_folded_box_keeps_its_header_and_the_labels_of_its_pieces() {
    let mut review = Pull::cart().open(json!([{"id": "cart", "title": "Cart", "why": "because",
        "pieces": [
            {"file": "app/Cart.php", "symbol": "Cart::total"},
            {"file": "app/Cart.php", "symbol": "Cart"},
        ], "children": []}]));
    let open = shapes(review.page());
    assert!(open.iter().any(|row| row.starts_with('+')));

    review.toggle_box_fold(&"cart".into(), &file_path("app/Cart.php"));

    assert_eq!(
        rows_from(review.page(), "header app/Cart.php", 4),
        [
            "header app/Cart.php",
            "marker app/Cart.php#Cart",
            "marker app/Cart.php#Cart::total",
            "end",
        ]
    );
}

#[test]
fn a_step_whose_pieces_fold_offers_the_fold_only_when_it_has_pieces() {
    let pull = Pull::new();
    let skim = |pieces: Value| {
        json!([{"id": "skim", "title": "Skim", "why": "because", "skim": true,
            "diagram": "graph TD; A-->B", "pieces": pieces, "children": []}])
    };
    let fold_of = |review: &Review| {
        review.page().rows().iter().find_map(|row| match row {
            PageRow::Toolbar(toolbar) => Some(toolbar.fold.is_some()),
            _ => None,
        })
    };

    assert_eq!(fold_of(&pull.open(skim(json!([])))), Some(false));
    assert_eq!(
        fold_of(&pull.open(skim(json!([{"file": "notes.txt"}])))),
        Some(true)
    );
}

#[test]
fn the_row_of_a_line_is_where_the_page_shows_it() {
    let review = Pull::new().open(two_steps());
    let page = review.page();

    let row = page.row_of_line(&at_head("notes.txt", 10)).unwrap();

    assert_eq!(page.line(row).unwrap().code.as_ref(), "line ten");
    assert_eq!(
        page.row_of_line(&at_head("other.txt", 2)),
        None,
        "on the next step"
    );
}

#[test]
fn a_thread_hung_on_the_page_shows_as_the_review_has_it_after_a_change() {
    let mut review = Pull::new().open(notes_step());
    review.set_threads(&[thread_on("T1", Some(10))]);
    let folded_before = thread_folded(review.page());

    review.toggle_thread_fold(&ThreadId::new("T1"));

    assert_eq!(thread_folded(review.page()), Some(!folded_before.unwrap()));
}

fn thread_folded(page: &Page) -> Option<bool> {
    page.rows().iter().find_map(|row| match row {
        PageRow::Thread {
            view,
            place: ThreadRowPlace::UnderLine,
            ..
        } => Some(view.is_folded),
        _ => None,
    })
}

fn pinned_at(page: &Page, row: usize) -> Option<(String, Option<String>)> {
    page.pinned_header(row).map(|pinned| {
        (
            pinned.header.path.to_string(),
            pinned.symbol.as_deref().map(str::to_owned),
        )
    })
}

fn cart_step() -> Value {
    json!([{"id": "cart", "title": "Cart", "why": "because",
        "pieces": [
            {"file": "app/Cart.php", "symbol": "Cart::total"},
            {"file": "app/Cart.php", "symbol": "Cart"},
        ], "children": []}])
}

#[test]
fn a_file_header_is_pinned_only_once_it_has_scrolled_away() {
    let review = Pull::new().open(notes_step());
    let page = review.page();
    let header = row_of(page, "header notes.txt");

    assert_eq!(pinned_at(page, header - 1), None, "above the box");
    assert_eq!(pinned_at(page, header), None, "the header is in view");
    assert_eq!(
        pinned_at(page, row_of(page, "+10")),
        Some(("notes.txt".to_owned(), None)),
        "a box of one piece names its symbol in its own header"
    );
    assert_eq!(
        page.pinned_header(row_of(page, "+10"))
            .map(|pinned| pinned.header_row),
        Some(header)
    );
    assert_eq!(pinned_at(page, row_of(page, "end")), None, "the box ends");
    assert_eq!(
        pinned_at(page, row_of(page, "footer")),
        None,
        "past the box"
    );
}

#[test]
fn a_pinned_header_names_the_symbol_the_top_row_is_in() {
    let review = Pull::cart().open(cart_step());
    let page = review.page();
    let in_cart = page.row_of_line(&at_head("app/Cart.php", 5)).unwrap();
    let in_total = page.row_of_line(&at_head("app/Cart.php", 11)).unwrap();

    assert_eq!(
        pinned_at(page, in_cart),
        Some(("app/Cart.php".to_owned(), Some("Cart".to_owned())))
    );
    assert_eq!(
        pinned_at(page, in_total),
        Some(("app/Cart.php".to_owned(), Some("Cart::total".to_owned())))
    );
}
