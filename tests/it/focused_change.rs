//! The focused change: which change of the screen the change keys act on.

use crate::support::{FixtureRepo, file_path, line};
use guided_review::local_git::LocalGit;
use guided_review::pull_request::RepoName;
use guided_review::tour::Tour;
use guided_review::viewer::{
    ChangeInView, LineKind, LineLocation, MoreLines, PageChange, PageRow, ReadStep, Review,
    ReviewStateFile, Side, hidden_lines_before,
};
use serde_json::{Value, json};
use std::fmt::Write;
use std::ops::Range;
use std::sync::Arc;

const PR: &str = "https://github.com/acme/shop/pull/7";

fn numbered(file: &str, change: impl Fn(usize) -> Option<&'static str>) -> String {
    (1..=40).fold(String::new(), |mut text, number| {
        match change(number) {
            Some(changed) => writeln!(text, "{changed}"),
            None => writeln!(text, "{file} {number}"),
        }
        .ok();
        text
    })
}

/// `notes.txt` changes on lines 10, 20 and 30, `other.txt` on line 5: four changes over two files,
/// far enough apart that none joins another.
struct Pull {
    repo: FixtureRepo,
    base: String,
    head: String,
}

impl Pull {
    fn new() -> Self {
        let repo = FixtureRepo::cloned_from("acme/shop");
        repo.write("notes.txt", &numbered("notes", |_| None));
        repo.write("other.txt", &numbered("other", |_| None));
        let base = repo.commit("base");
        repo.write(
            "notes.txt",
            &numbered("notes", |number| match number {
                10 => Some("ten"),
                20 => Some("twenty"),
                30 => Some("thirty"),
                _ => None,
            }),
        );
        repo.write(
            "other.txt",
            &numbered("other", |number| (number == 5).then_some("five")),
        );
        let head = repo.commit("head");
        Self { repo, base, head }
    }

    /// Opens the tour on its first step, with the files at head read.
    fn open(&self, steps: &Value) -> Review {
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
        review
    }

    fn open_on_the_code(&self) -> Review {
        let mut review = self.open(&json!([{
            "id": "notes", "title": "Notes", "why": "because", "children": [],
            "pieces": [{"file": "notes.txt"}, {"file": "other.txt"}],
        }]));
        review.go_forward();
        review
    }
}

/// The text of the first added line of the focused change; `None` with no focus.
fn focused(review: &Review) -> Option<String> {
    let page = review.page();
    let change = page.focused_change()?;
    change.rows().find_map(|row| match page.row(row)? {
        PageRow::Line(line) if line.kind == LineKind::Added => Some(line.code.to_string()),
        _ => None,
    })
}

#[test]
fn a_screen_with_code_has_one_change_per_changed_hunk_in_page_order() {
    let review = Pull::new().open_on_the_code();

    assert_eq!(review.page().changes().len(), 4);
    assert_eq!(focused(&review), None);
}

#[test]
fn tab_focuses_the_first_change_when_none_has_the_focus() {
    let mut review = Pull::new().open_on_the_code();

    let row = review.focus_next_change();

    assert_eq!(focused(&review).as_deref(), Some("ten"));
    assert_eq!(row, Some(review.page().changes()[0].first_row()));
}

#[test]
fn tab_walks_the_changes_of_the_screen_across_its_files_and_shift_tab_walks_back() {
    let mut review = Pull::new().open_on_the_code();
    let mut seen = Vec::new();

    for _ in 0..4 {
        review.focus_next_change();
        seen.push(focused(&review).unwrap());
    }
    assert_eq!(seen, ["ten", "twenty", "thirty", "five"]);

    for _ in 0..3 {
        review.focus_previous_change();
        seen.push(focused(&review).unwrap());
    }
    assert_eq!(seen[4..], ["thirty", "twenty", "ten"]);
}

#[test]
fn tab_from_the_last_change_wraps_to_the_first_and_shift_tab_from_the_first_to_the_last() {
    let mut review = Pull::new().open_on_the_code();
    for _ in 0..4 {
        review.focus_next_change();
    }
    assert_eq!(focused(&review).as_deref(), Some("five"));

    review.focus_next_change();
    assert_eq!(focused(&review).as_deref(), Some("ten"));

    review.focus_previous_change();
    assert_eq!(focused(&review).as_deref(), Some("five"));
}

#[test]
fn shift_tab_with_no_focus_goes_to_the_last_change() {
    let mut review = Pull::new().open_on_the_code();

    review.focus_previous_change();

    assert_eq!(focused(&review).as_deref(), Some("five"));
}

#[test]
fn a_screen_with_no_code_has_no_focus_to_move() {
    let mut review = Pull::new().open_on_the_code();
    review.focus_next_change();
    review.go_back();

    assert_eq!(review.focus_next_change(), None);
    assert_eq!(review.focus_previous_change(), None);
    assert_eq!(focused(&review), None);
}

#[test]
fn the_focus_is_not_kept_for_a_screen_it_is_not_on() {
    let mut review = Pull::new().open(&json!([
        {"id": "notes", "title": "Notes", "why": "because", "children": [],
         "pieces": [{"file": "notes.txt"}]},
        {"id": "other", "title": "Other", "why": "because", "children": [],
         "pieces": [{"file": "other.txt"}]},
    ]));
    review.go_forward();
    review.focus_next_change();

    review.go_forward();

    assert_eq!(focused(&review), None);
    review.focus_next_change();
    assert_eq!(focused(&review).as_deref(), Some("five"));
}

/// Every line the first piece shows, as `+10`, `-10` or ` 10`: what the box has on it.
fn shown(review: &Review) -> Vec<String> {
    review.current_step().unwrap().pieces[0]
        .hunks
        .iter()
        .flat_map(|hunk| &hunk.rows)
        .map(|row| {
            let sign = match row.kind {
                LineKind::Added => '+',
                LineKind::Removed => '-',
                LineKind::Context { .. } => ' ',
            };
            format!("{sign}{}", row.location.line)
        })
        .collect()
}

fn focus_change_number(review: &mut Review, number: usize) {
    for _ in 0..number {
        review.focus_next_change();
    }
}

#[test]
fn the_key_below_shows_what_the_gap_button_under_the_change_shows() {
    let pull = Pull::new();
    let mut by_key = pull.open_on_the_code();
    focus_change_number(&mut by_key, 1);
    let mut by_button = pull.open_on_the_code();
    let hunks = by_button.current_step().unwrap().pieces[0].hunks.clone();
    let button = hidden_lines_before(&hunks, 1).unwrap().after_above.unwrap();

    assert!(by_key.show_more_below());
    by_button.display_options_mut().show_more_context(&button);

    assert_eq!(shown(&by_key), shown(&by_button));
    assert!(shown(&by_key).contains(&" 33".to_owned()));
}

#[test]
fn the_key_above_shows_what_the_gap_button_over_the_change_shows() {
    let pull = Pull::new();
    let mut by_key = pull.open_on_the_code();
    focus_change_number(&mut by_key, 2);
    let mut by_button = pull.open_on_the_code();
    let hunks = by_button.current_step().unwrap().pieces[0].hunks.clone();
    let button = hidden_lines_before(&hunks, 1)
        .unwrap()
        .before_below
        .unwrap();

    assert!(by_key.show_more_above());
    by_button.display_options_mut().show_more_context(&button);

    assert_eq!(shown(&by_key), shown(&by_button));
    assert!(shown(&by_key).contains(&" 7".to_owned()));
}

#[test]
fn the_keys_and_the_gap_buttons_show_the_number_of_lines_the_reviewer_set() {
    let pull = Pull::new();
    let two = MoreLines::new(2).unwrap();
    let mut by_key = pull.open_on_the_code();
    by_key.display_options_mut().use_more_lines(two);
    focus_change_number(&mut by_key, 1);
    let mut by_button = pull.open_on_the_code();
    by_button.display_options_mut().use_more_lines(two);
    let hunks = by_button.current_step().unwrap().pieces[0].hunks.clone();
    let button = hidden_lines_before(&hunks, 1).unwrap().after_above.unwrap();

    assert!(by_key.show_more_below());
    by_button.display_options_mut().show_more_context(&button);

    assert_eq!(shown(&by_key), shown(&by_button));
    assert!(shown(&by_key).contains(&" 15".to_owned()));
    assert!(!shown(&by_key).contains(&" 16".to_owned()));
}

#[test]
fn the_key_above_shows_the_number_of_lines_the_reviewer_set() {
    let mut review = Pull::new().open_on_the_code();
    review
        .display_options_mut()
        .use_more_lines(MoreLines::new(2).unwrap());
    focus_change_number(&mut review, 2);

    assert!(review.show_more_above());

    assert!(shown(&review).contains(&" 15".to_owned()));
    assert!(!shown(&review).contains(&" 14".to_owned()));
}

#[test]
fn the_focus_stays_on_its_change_when_more_lines_join_it_to_the_next() {
    let mut review = Pull::new().open_on_the_code();
    focus_change_number(&mut review, 2);
    assert_eq!(focused_additions(&review), ["twenty"]);

    review.show_more_above();

    assert_eq!(focused_additions(&review), ["ten", "twenty"]);
    review.show_more_below();
    assert_eq!(focused_additions(&review), ["ten", "twenty", "thirty"]);
}

fn focused_additions(review: &Review) -> Vec<String> {
    let page = review.page();
    let rows = page.focused_change().map(PageChange::rows);
    rows.into_iter()
        .flatten()
        .filter_map(|row| match page.row(row)? {
            PageRow::Line(line) if line.kind == LineKind::Added => Some(line.code.to_string()),
            _ => None,
        })
        .collect()
}

#[test]
fn the_keys_do_nothing_with_no_focused_change() {
    let mut review = Pull::new().open_on_the_code();
    let before = shown(&review);

    assert!(!review.show_more_above());
    assert!(!review.show_more_below());

    assert_eq!(shown(&review), before);
}

#[test]
fn the_key_does_nothing_where_the_file_has_no_more_lines_on_that_edge() {
    let mut review = Pull::new().open_on_the_code();
    focus_change_number(&mut review, 4);
    assert!(review.show_more_above());
    let before = shown(&review);

    assert!(!review.show_more_above());

    assert_eq!(shown(&review), before);
    assert_eq!(focused(&review).as_deref(), Some("five"));
}

fn row_of(review: &Review, path: &str, number: usize) -> usize {
    review
        .page()
        .row_of_line(&LineLocation {
            path: file_path(path),
            side: Side::Head,
            line: line(number),
        })
        .unwrap()
}

#[test]
fn a_click_on_a_line_focuses_the_change_it_is_in_unchanged_lines_around_it_too() {
    let mut review = Pull::new().open_on_the_code();

    assert!(review.focus_change_at_row(row_of(&review, "notes.txt", 22)));
    assert_eq!(focused(&review).as_deref(), Some("twenty"));

    assert!(review.focus_change_at_row(row_of(&review, "other.txt", 5)));
    assert_eq!(focused(&review).as_deref(), Some("five"));
}

#[test]
fn a_click_outside_every_change_leaves_the_focus_alone() {
    let mut review = Pull::new().open_on_the_code();
    focus_change_number(&mut review, 2);

    assert!(!review.focus_change_at_row(0));

    assert_eq!(focused(&review).as_deref(), Some("twenty"));
}

#[test]
fn a_click_in_the_focused_change_changes_nothing_so_the_page_is_kept() {
    let mut review = Pull::new().open_on_the_code();
    review.focus_change_at_row(row_of(&review, "notes.txt", 20));
    let page = Arc::clone(review.page());

    assert!(!review.focus_change_at_row(row_of(&review, "notes.txt", 21)));

    assert!(Arc::ptr_eq(&page, review.page()));
}

/// The rows of the screen as they are: the change at `at` of the page's changes.
fn rows_of_change(review: &Review, at: usize) -> Range<usize> {
    review.page().changes()[at].rows()
}

#[test]
fn a_focus_that_is_in_view_stays_where_it_is() {
    let mut review = Pull::new().open_on_the_code();
    focus_change_number(&mut review, 2);
    let page = Arc::clone(review.page());
    let all = 0..page.len();

    assert!(!review.follow_scroll(all));

    assert_eq!(focused(&review).as_deref(), Some("twenty"));
    assert!(Arc::ptr_eq(&page, review.page()));
}

#[test]
fn a_focus_that_scrolled_out_of_view_goes_to_the_first_change_in_view() {
    let mut review = Pull::new().open_on_the_code();
    focus_change_number(&mut review, 1);
    let visible = rows_of_change(&review, 2).start..rows_of_change(&review, 3).end;

    assert!(review.follow_scroll(visible));

    assert_eq!(focused(&review).as_deref(), Some("thirty"));
}

#[test]
fn a_change_with_one_row_in_view_counts_as_in_view() {
    let mut review = Pull::new().open_on_the_code();
    focus_change_number(&mut review, 1);
    let last_row = rows_of_change(&review, 0).end - 1;

    assert!(!review.follow_scroll(last_row..last_row + 1));

    assert_eq!(focused(&review).as_deref(), Some("ten"));
}

#[test]
fn with_no_change_in_view_the_focus_stays() {
    let mut review = Pull::new().open_on_the_code();
    focus_change_number(&mut review, 1);

    assert!(!review.follow_scroll(0..1));

    assert_eq!(focused(&review).as_deref(), Some("ten"));
}

#[test]
fn a_screen_with_code_and_no_focus_gives_it_to_the_first_change_in_view() {
    let mut review = Pull::new().open_on_the_code();
    let visible = rows_of_change(&review, 1).start..review.page().len();

    assert!(review.follow_scroll(visible));

    assert_eq!(focused(&review).as_deref(), Some("twenty"));
}

#[test]
fn a_screen_with_no_code_gets_no_focus_from_the_scroll() {
    let mut review = Pull::new().open_on_the_code();
    review.go_back();
    let all = 0..review.page().len();

    assert!(!review.follow_scroll(all));

    assert_eq!(focused(&review), None);
}

/// A change of 250 pixels in a view of 100.
fn tall(top: f32, bottom: f32) -> ChangeInView {
    ChangeInView {
        view_height: 100.0,
        top,
        bottom,
    }
}

#[test]
fn down_scrolls_a_change_taller_than_the_view_until_its_end_shows() {
    let mut review = Pull::new().open_on_the_code();
    focus_change_number(&mut review, 2);

    assert_eq!(
        review.read_down(Some(tall(0.0, 250.0))),
        ReadStep::Scroll(80.0)
    );
    assert_eq!(
        review.read_down(Some(tall(-80.0, 170.0))),
        ReadStep::Scroll(70.0)
    );
    assert_eq!(focused(&review).as_deref(), Some("twenty"));
}

#[test]
fn down_moves_on_to_the_next_change_once_the_end_of_a_tall_one_shows() {
    let mut review = Pull::new().open_on_the_code();
    focus_change_number(&mut review, 2);

    let step = review.read_down(Some(tall(-150.0, 100.0)));

    assert_eq!(
        step,
        ReadStep::Focused(review.page().changes()[2].first_row())
    );
    assert_eq!(focused(&review).as_deref(), Some("thirty"));
}

#[test]
fn down_moves_on_from_a_change_that_fits_the_view_even_when_it_is_cut_off() {
    let mut review = Pull::new().open_on_the_code();
    focus_change_number(&mut review, 2);

    let step = review.read_down(Some(tall(60.0, 140.0)));

    assert!(matches!(step, ReadStep::Focused(_)));
    assert_eq!(focused(&review).as_deref(), Some("thirty"));
}

#[test]
fn down_with_no_measure_of_the_change_moves_on() {
    let mut review = Pull::new().open_on_the_code();
    focus_change_number(&mut review, 2);

    assert!(matches!(review.read_down(None), ReadStep::Focused(_)));
    assert_eq!(focused(&review).as_deref(), Some("thirty"));
}

#[test]
fn up_scrolls_a_change_taller_than_the_view_until_its_start_shows() {
    let mut review = Pull::new().open_on_the_code();
    focus_change_number(&mut review, 2);

    assert_eq!(
        review.read_up(Some(tall(-150.0, 100.0))),
        ReadStep::Scroll(-80.0)
    );
    assert_eq!(
        review.read_up(Some(tall(-30.0, 220.0))),
        ReadStep::Scroll(-30.0)
    );
    assert_eq!(focused(&review).as_deref(), Some("twenty"));
}

#[test]
fn up_moves_back_to_the_previous_change_once_the_start_of_a_tall_one_shows() {
    let mut review = Pull::new().open_on_the_code();
    focus_change_number(&mut review, 2);

    let step = review.read_up(Some(tall(0.0, 250.0)));

    assert_eq!(
        step,
        ReadStep::Focused(review.page().changes()[0].first_row())
    );
    assert_eq!(focused(&review).as_deref(), Some("ten"));
}

#[test]
fn down_at_the_last_change_and_up_at_the_first_do_nothing() {
    let mut review = Pull::new().open_on_the_code();
    focus_change_number(&mut review, 4);
    assert_eq!(review.read_down(None), ReadStep::Nothing);
    assert_eq!(focused(&review).as_deref(), Some("five"));

    focus_change_number(&mut review, 1);
    assert_eq!(review.read_up(None), ReadStep::Nothing);
    assert_eq!(focused(&review).as_deref(), Some("ten"));
}

#[test]
fn down_and_up_with_no_focus_focus_the_first_and_the_last_change() {
    let pull = Pull::new();
    let mut review = pull.open_on_the_code();
    assert!(matches!(review.read_down(None), ReadStep::Focused(_)));
    assert_eq!(focused(&review).as_deref(), Some("ten"));

    let mut review = pull.open_on_the_code();
    assert!(matches!(review.read_up(None), ReadStep::Focused(_)));
    assert_eq!(focused(&review).as_deref(), Some("five"));
}

#[test]
fn down_and_up_on_a_screen_with_no_code_do_nothing() {
    let mut review = Pull::new().open_on_the_code();
    review.go_back();

    assert_eq!(review.read_down(None), ReadStep::Nothing);
    assert_eq!(review.read_up(None), ReadStep::Nothing);
}
