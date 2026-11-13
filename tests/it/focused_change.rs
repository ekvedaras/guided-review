//! The focused change: which change of the screen the change keys act on.

use crate::support::FixtureRepo;
use guided_review::local_git::LocalGit;
use guided_review::pull_request::RepoName;
use guided_review::tour::Tour;
use guided_review::viewer::{
    LineKind, PageChange, PageRow, Review, ReviewStateFile, hidden_lines_before,
};
use serde_json::{Value, json};
use std::fmt::Write;

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
