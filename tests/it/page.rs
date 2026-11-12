//! The page of the open screen: built once per change of the review and kept until the next one.

use std::sync::Arc;

use crate::support::{FixtureRepo, file_path, line};
use guided_review::local_git::LocalGit;
use guided_review::pull_request::RepoName;
use guided_review::tour::Tour;
use guided_review::viewer::{LineLocation, Review, ReviewStateFile, Side};
use serde_json::{Value, json};

const PR: &str = "https://github.com/acme/shop/pull/7";

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
