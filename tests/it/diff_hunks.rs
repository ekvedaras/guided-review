use crate::support::FixtureRepo;
use guided_review::local_git::LocalGit;
use guided_review::pull_request::RepoName;
use guided_review::tour::Tour;
use guided_review::viewer::{
    DiffHunk, FileBlock, LineKind, LineLocation, Review, ReviewStateFile, Side, hidden_lines_after,
    hidden_lines_before,
};
use serde_json::{Value, json};
use std::fmt::Write;

fn numbered_lines(count: usize, change: impl Fn(usize) -> Option<String>) -> String {
    (1..=count)
        .map(|number| change(number).unwrap_or_else(|| format!("line {number}")))
        .filter(|text| text != "<gone>")
        .fold(String::new(), |mut lines, text| {
            writeln!(lines, "{text}").ok();
            lines
        })
}

struct Pull {
    repo: FixtureRepo,
    base: String,
    head: String,
}

impl Pull {
    fn changing(path: &str, base: &str, head: &str) -> Self {
        let repo = FixtureRepo::cloned_from("acme/shop");
        repo.write(path, base);
        let base = repo.commit("base");
        repo.write(path, head);
        let head = repo.commit("head");
        Self { repo, base, head }
    }

    /// The step `notes` shows the whole changed file; its text at head is already read.
    fn open_step(&self, pieces: Value) -> Review {
        let tour: Tour = serde_json::from_value(json!({
            "pr": "https://github.com/acme/shop/pull/7",
            "base": self.base,
            "head": self.head,
            "title": "Notes",
            "summary": [],
            "steps": [{"id": "notes", "title": "Notes", "why": "because", "pieces": pieces, "children": []}],
        }))
        .unwrap();
        let source = LocalGit::find(&RepoName::new("acme", "shop"), self.repo.path())
            .unwrap()
            .unwrap();
        let state_file = ReviewStateFile::for_pr(
            self.repo.home(),
            &"https://github.com/acme/shop/pull/7".parse().unwrap(),
        );
        let mut review = Review::open(tour, &source, state_file).unwrap();
        let files_at_head = review.files_at_head_request().read(&source);
        review.set_files_at_head(files_at_head);
        review.go_forward();
        review
    }
}

/// `-10` removed at base, `+10` added at head, ` 12|13` context at base 12 and head 13, `~` before an expanded line.
fn rows(hunks: &[DiffHunk]) -> Vec<String> {
    hunks
        .iter()
        .flat_map(|hunk| {
            std::iter::once("@@".to_owned()).chain(hunk.rows.iter().map(|row| {
                let number = row.location.line;
                match row.kind {
                    LineKind::Removed => format!("-{number} {}", row.text),
                    LineKind::Added => format!("+{number} {}", row.text),
                    LineKind::Context { base_line, .. } => format!(
                        "{} {}|{number} {}",
                        if row.is_inside_hunk() { "" } else { "~" },
                        base_line.map_or_default(|line| line.to_string()),
                        row.text
                    ),
                }
            }))
        })
        .collect()
}

fn shown_hunks(review: &Review) -> Vec<DiffHunk> {
    review.current_step().unwrap().pieces.remove(0).hunks
}

/// Line 10 and 14 change, line 30 is deleted.
fn three_changes() -> Pull {
    Pull::changing(
        "notes.txt",
        &numbered_lines(40, |_| None),
        &numbered_lines(40, |number| match number {
            10 => Some("line ten".into()),
            14 => Some("line fourteen".into()),
            30 => Some("<gone>".into()),
            _ => None,
        }),
    )
}

#[test]
fn a_piece_shows_each_change_with_its_removed_and_added_lines_together_and_three_lines_around() {
    let review = three_changes().open_step(json!([{"file": "notes.txt"}]));

    assert_eq!(
        rows(&shown_hunks(&review)),
        [
            "@@",
            " 7|7 line 7",
            " 8|8 line 8",
            " 9|9 line 9",
            "-10 line 10",
            "+10 line ten",
            " 11|11 line 11",
            " 12|12 line 12",
            " 13|13 line 13",
            "-14 line 14",
            "+14 line fourteen",
            " 15|15 line 15",
            " 16|16 line 16",
            " 17|17 line 17",
            "@@",
            " 27|27 line 27",
            " 28|28 line 28",
            " 29|29 line 29",
            "-30 line 30",
            " 31|30 line 31",
            " 32|31 line 32",
            " 33|32 line 33",
        ]
    );
}

#[test]
fn more_context_above_reads_the_lines_at_head_and_they_take_no_comment() {
    let mut review = three_changes().open_step(json!([{"file": "notes.txt"}]));
    let more = shown_hunks(&review)[0].more_above.clone().unwrap();

    review.display_options_mut().show_more_context(&more);

    let hunks = shown_hunks(&review);
    assert_eq!(
        rows(&hunks)[..9],
        [
            "@@",
            "~ 1|1 line 1",
            "~ 2|2 line 2",
            "~ 3|3 line 3",
            "~ 4|4 line 4",
            "~ 5|5 line 5",
            "~ 6|6 line 6",
            " 7|7 line 7",
            " 8|8 line 8",
        ]
    );
    assert!(
        hunks[0].more_above.is_none(),
        "nothing is left above line 1"
    );
}

#[test]
fn more_context_below_joins_a_change_with_the_next_one_when_the_gap_is_shown() {
    let mut review = three_changes().open_step(json!([{"file": "notes.txt"}]));
    let more = shown_hunks(&review)[0].more_below.clone().unwrap();

    review.display_options_mut().show_more_context(&more);

    let hunks = shown_hunks(&review);
    assert_eq!(hunks.len(), 1);
    assert_eq!(
        rows(&hunks)[13..],
        [
            " 17|17 line 17",
            "~ 18|18 line 18",
            "~ 19|19 line 19",
            "~ 20|20 line 20",
            "~ 21|21 line 21",
            "~ 22|22 line 22",
            "~ 23|23 line 23",
            "~ 24|24 line 24",
            "~ 25|25 line 25",
            "~ 26|26 line 26",
            " 27|27 line 27",
            " 28|28 line 28",
            " 29|29 line 29",
            "-30 line 30",
            " 31|30 line 31",
            " 32|31 line 32",
            " 33|32 line 33",
        ]
    );
    assert!(hunks[0].more_above.is_some());
    assert!(hunks[0].more_below.is_some());
}

fn shown_blocks(review: &Review) -> Vec<FileBlock> {
    FileBlock::group(review.current_step().unwrap().pieces)
}

#[test]
fn parts_of_one_file_show_in_one_block_in_line_order_whatever_order_the_step_names_them() {
    let review = three_changes().open_step(json!([
        {"file": "notes.txt", "lines": [14, 14]},
        {"file": "notes.txt", "lines": [10, 10]},
    ]));

    let blocks = shown_blocks(&review);

    assert_eq!(blocks.len(), 1);
    assert_eq!(
        rows(blocks[0].hunks()),
        [
            "@@",
            "-10 line 10",
            "+10 line ten",
            "@@",
            "-14 line 14",
            "+14 line fourteen",
        ]
    );
}

#[test]
fn a_row_two_parts_of_a_block_both_show_appears_once() {
    let review = three_changes().open_step(json!([
        {"file": "notes.txt", "lines": [10, 16]},
        {"file": "notes.txt", "lines": [7, 12]},
    ]));

    let blocks = shown_blocks(&review);

    assert_eq!(
        rows(blocks[0].hunks()),
        [
            "@@",
            " 7|7 line 7",
            " 8|8 line 8",
            " 9|9 line 9",
            "-10 line 10",
            "+10 line ten",
            " 11|11 line 11",
            " 12|12 line 12",
            " 13|13 line 13",
            "-14 line 14",
            "+14 line fourteen",
            " 15|15 line 15",
            " 16|16 line 16",
        ]
    );
}

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

/// A property joins the class at head line 5, and `$d` changes in the middle of `total` at head line 11.
fn cart_changes() -> Pull {
    Pull::changing(
        "app/Cart.php",
        CART_AT_BASE,
        &CART_AT_BASE
            .replace(
                "{\n    public function total",
                "{\n    private int $items = 0;\n    public function total",
            )
            .replace("$d = 4;", "$d = 5;"),
    )
}

fn label_at(block: &FileBlock, head_line: usize) -> Option<(String, bool)> {
    let at = LineLocation {
        path: block.path.clone(),
        side: Side::Head,
        line: crate::support::line(head_line),
    };
    block
        .piece_starting_at(&at)
        .map(|(piece, is_resumed)| (piece.id.to_string(), is_resumed))
}

#[test]
fn a_label_in_a_block_sits_where_its_method_starts_not_at_its_first_changed_line() {
    let review = cart_changes().open_step(json!([
        {"file": "app/Cart.php", "symbol": "Cart::total"},
        {"file": "app/Cart.php", "symbol": "Cart"},
    ]));

    let block = shown_blocks(&review).remove(0);

    assert_eq!(
        label_at(&block, 3),
        Some(("app/Cart.php#Cart".into(), false))
    );
    assert_eq!(
        label_at(&block, 6),
        Some(("app/Cart.php#Cart::total".into(), false))
    );
    assert_eq!(label_at(&block, 11), None);
    assert_eq!(
        label_at(&block, 13),
        None,
        "a closing bracket moves no label"
    );
}

/// Line 5 only gets indented; lines 20 and 21 change, 21 only in its indent.
fn whitespace_changes() -> Pull {
    Pull::changing(
        "notes.txt",
        &numbered_lines(30, |_| None),
        &numbered_lines(30, |number| match number {
            5 => Some("    line 5".into()),
            20 => Some("line twenty".into()),
            21 => Some("  line 21".into()),
            _ => None,
        }),
    )
}

#[test]
fn ignoring_whitespace_shows_lines_that_only_change_whitespace_as_unchanged() {
    let mut review = whitespace_changes().open_step(json!([{"file": "notes.txt"}]));

    review.display_options_mut().toggle_ignore_whitespace();

    assert!(review.display_options().ignores_whitespace());
    assert_eq!(
        rows(&shown_hunks(&review)),
        [
            "@@",
            " 17|17 line 17",
            " 18|18 line 18",
            " 19|19 line 19",
            "-20 line 20",
            "+20 line twenty",
            " 21|21   line 21",
            " 22|22 line 22",
            " 23|23 line 23",
            " 24|24 line 24",
        ]
    );
    review.display_options_mut().toggle_ignore_whitespace();
    assert_eq!(shown_hunks(&review).len(), 2);
}

#[test]
fn a_piece_that_changes_only_whitespace_says_so_when_whitespace_is_ignored() {
    let mut review =
        Pull::changing("notes.txt", "a\nb\n", "a\n  b\n").open_step(json!([{"file": "notes.txt"}]));
    assert!(!review.current_step().unwrap().pieces[0].changes_only_whitespace);

    review.display_options_mut().toggle_ignore_whitespace();

    let piece = review.current_step().unwrap().pieces.remove(0);
    assert!(piece.hunks.is_empty());
    assert!(piece.changes_only_whitespace);
}

#[test]
fn ignoring_whitespace_changes_no_piece_and_no_read_tick() {
    let mut review = whitespace_changes().open_step(json!([{"rest": ""}]));
    let piece_ids = |review: &Review| -> Vec<String> {
        review
            .current_step()
            .unwrap()
            .pieces
            .iter()
            .map(|piece| piece.id.to_string())
            .collect()
    };
    review.tick(&"notes".into()).unwrap();
    let before = (piece_ids(&review), review.progress());

    review.display_options_mut().toggle_ignore_whitespace();

    assert_eq!((piece_ids(&review), review.progress()), before);
    assert!(review.is_ticked(&"notes".into()));
}

#[test]
fn the_lines_between_two_changes_are_one_gap_that_says_how_many_are_hidden() {
    let review = three_changes().open_step(json!([{"file": "notes.txt"}]));
    let hunks = shown_hunks(&review);

    let between = hidden_lines_before(&hunks, 1).unwrap();
    assert_eq!(between.count, Some(9), "lines 18 to 26");
    assert!(between.after_above.is_some());
    assert!(between.before_below.is_some());

    let top = hidden_lines_before(&hunks, 0).unwrap();
    assert_eq!(top.count, Some(6), "lines 1 to 6");
    assert!(
        top.after_above.is_none(),
        "nothing is above the top of the file"
    );
    assert!(top.before_below.is_some());

    let end = hidden_lines_after(&hunks).unwrap();
    assert!(end.after_above.is_some());
    assert!(
        end.before_below.is_none(),
        "nothing is below the end of the file"
    );
}

#[test]
fn a_change_at_the_top_of_the_file_has_no_gap_above_it() {
    let review = Pull::changing(
        "notes.txt",
        &numbered_lines(20, |_| None),
        &numbered_lines(20, |number| (number == 1).then(|| "line one".into())),
    )
    .open_step(json!([{"file": "notes.txt"}]));

    assert_eq!(hidden_lines_before(&shown_hunks(&review), 0), None);
}

#[test]
fn showing_every_hidden_line_of_a_gap_joins_the_changes_around_it() {
    let mut review = Pull::changing(
        "notes.txt",
        &numbered_lines(100, |_| None),
        &numbered_lines(100, |number| match number {
            10 => Some("line ten".into()),
            80 => Some("line eighty".into()),
            _ => None,
        }),
    )
    .open_step(json!([{"file": "notes.txt"}]));
    let between = hidden_lines_before(&shown_hunks(&review), 1).unwrap();
    assert_eq!(between.count, Some(63), "lines 14 to 76");

    review.display_options_mut().show_hidden_lines(&between);

    let hunks = shown_hunks(&review);
    assert_eq!(hunks.len(), 1);
    assert!(rows(&hunks).contains(&"~ 45|45 line 45".to_owned()));
}

/// The words each changed row marks, as text: `-` rows for the base side, `+` rows for head.
fn marked_words(review: &Review) -> Vec<String> {
    shown_hunks(review)
        .iter()
        .flat_map(|hunk| &hunk.rows)
        .filter(|row| !matches!(row.kind, LineKind::Context { .. }))
        .map(|row| {
            let sign = if row.kind == LineKind::Removed {
                '-'
            } else {
                '+'
            };
            let words: Vec<&str> = row
                .changed
                .iter()
                .filter_map(|range| row.text.get(range.clone()))
                .collect();
            format!("{sign} {}", words.join(" | "))
        })
        .collect()
}

fn one_line_changing(base: &str, head: &str) -> Review {
    Pull::changing(
        "notes.txt",
        &format!("top\n{base}\nend\n"),
        &format!("top\n{head}\nend\n"),
    )
    .open_step(json!([{"file": "notes.txt"}]))
}

#[test]
fn a_changed_line_marks_the_words_that_changed_on_both_sides() {
    let review = one_line_changing("the cart holds three items", "the cart holds four items");

    assert_eq!(marked_words(&review), ["- three", "+ four"]);
}

#[test]
fn words_taken_out_of_a_line_are_marked_as_one_run_on_the_removed_side() {
    let review = one_line_changing(
        "one of its order items, as a product return's extra, or as a pool line's remainder",
        "one of its order items, or as a pool line's remainder",
    );

    assert_eq!(
        marked_words(&review),
        ["- as a product return's extra,", "+ "]
    );
}

#[test]
fn a_line_written_anew_marks_no_words_as_it_is_all_changed() {
    let review = one_line_changing("alpha beta gamma", "one two three four");

    assert_eq!(marked_words(&review), ["- ", "+ "]);
}
