#[allow(dead_code)]
mod support;

use guided_review::local_git::LocalGit;
use guided_review::pull_request::RepoName;
use guided_review::tour::Tour;
use guided_review::viewer::{DiffHunk, LineKind, Review, ReviewStateFile};
use serde_json::{Value, json};
use support::FixtureRepo;

fn numbered_lines(count: usize, change: impl Fn(usize) -> Option<String>) -> String {
    (1..=count)
        .map(|number| change(number).unwrap_or_else(|| format!("line {number}")))
        .filter(|text| text != "<gone>")
        .map(|text| format!("{text}\n"))
        .collect()
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
            std::iter::once("@@".to_string()).chain(hunk.rows.iter().map(|row| {
                let number = row.location.line;
                match row.kind {
                    LineKind::Removed => format!("-{number} {}", row.text),
                    LineKind::Added => format!("+{number} {}", row.text),
                    LineKind::Context { base_line, .. } => format!(
                        "{} {base_line}|{number} {}",
                        if row.is_inside_hunk() { "" } else { "~" },
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

    review.show_more_context(&more);

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

    review.show_more_context(&more);

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

    review.toggle_ignore_whitespace();

    assert!(review.ignores_whitespace());
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
    review.toggle_ignore_whitespace();
    assert_eq!(shown_hunks(&review).len(), 2);
}

#[test]
fn a_piece_that_changes_only_whitespace_says_so_when_whitespace_is_ignored() {
    let mut review =
        Pull::changing("notes.txt", "a\nb\n", "a\n  b\n").open_step(json!([{"file": "notes.txt"}]));
    assert!(!review.current_step().unwrap().pieces[0].changes_only_whitespace);

    review.toggle_ignore_whitespace();

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

    review.toggle_ignore_whitespace();

    assert_eq!((piece_ids(&review), review.progress()), before);
    assert!(review.is_ticked(&"notes".into()));
}
