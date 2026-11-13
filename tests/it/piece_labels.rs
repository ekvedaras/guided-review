//! Piece labels per file: the thin `Class::method` lines that mark where each piece starts, hidden
//! or shown per file, and the keys `R` and `L` that act on the focused change's file.

use crate::support::{FixtureRepo, file_path};
use guided_review::local_git::LocalGit;
use guided_review::pull_request::RepoName;
use guided_review::tour::Tour;
use guided_review::update::Build;
use guided_review::viewer::{
    Chord, Command, Effect, FileToggle, FileView, Intent, KeyScope, PageRow, PaletteAction,
    PieceLabels, Review, ReviewStateFile, ShortcutGroup, WindowMoment,
};
use serde_json::{Value, json};
use std::fmt::Write;

const PR: &str = "https://github.com/acme/shop/pull/7";

fn php(methods: &[(&str, &[&str])]) -> String {
    let mut body = String::new();
    for (name, lines) in methods {
        writeln!(body, "    public function {name}(): int\n    {{").unwrap();
        for line in *lines {
            writeln!(body, "        {line}").unwrap();
        }
        writeln!(body, "    }}\n").unwrap();
    }
    format!("<?php\n\nfinal class Cart\n{{\n{}\n}}\n", body.trim_end())
}

struct Pull {
    repo: FixtureRepo,
    base: String,
    head: String,
}

impl Pull {
    /// `app/Tiny.php` changes one line in each of three methods; `app/Large.php` changes four
    /// lines in each of two; `README.md` changes a paragraph.
    fn new() -> Self {
        let repo = FixtureRepo::cloned_from("acme/shop");
        let tiny = |value: &str| -> String {
            php(&[
                ("a", &[&format!("return {value}1;")]),
                ("b", &[&format!("return {value}2;")]),
                ("c", &[&format!("return {value}3;")]),
            ])
        };
        let large = |value: &str| -> String {
            let lines = [
                format!("$a = {value}1;"),
                format!("$b = {value}2;"),
                format!("$c = {value}3;"),
                format!("$d = {value}4;"),
                "return 0;".to_owned(),
            ];
            let lines: Vec<&str> = lines.iter().map(String::as_str).collect();
            php(&[("x", &lines), ("y", &lines)])
        };
        repo.write("app/Tiny.php", &tiny(""));
        repo.write("app/Large.php", &large(""));
        repo.write("README.md", "# Shop\n\nSells things.\n");
        let base = repo.commit("base");
        repo.write("app/Tiny.php", &tiny("9"));
        repo.write("app/Large.php", &large("9"));
        repo.write("README.md", "# Shop\n\nSells many things.\n");
        let head = repo.commit("head");
        Self { repo, base, head }
    }

    fn open(&self) -> Review {
        let symbols = |file: &str, names: &[&str]| -> Vec<Value> {
            names
                .iter()
                .map(|name| json!({"file": file, "symbol": format!("Cart::{name}")}))
                .collect()
        };
        let mut pieces = symbols("app/Tiny.php", &["a", "b", "c"]);
        pieces.extend(symbols("app/Large.php", &["x", "y"]));
        pieces.push(json!({"file": "README.md"}));
        self.open_with(pieces)
    }

    fn open_pieces(&self, symbols: &[(&str, &str)]) -> Review {
        self.open_with(
            symbols
                .iter()
                .map(|(file, symbol)| json!({"file": file, "symbol": symbol}))
                .collect(),
        )
    }

    fn open_with(&self, pieces: Vec<Value>) -> Review {
        let tour: Tour = serde_json::from_value(json!({
            "pr": PR, "base": self.base, "head": self.head, "title": "Cart", "summary": [],
            "steps": [{"id": "cart", "title": "Cart", "why": "because", "pieces": pieces,
                "children": []}],
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

fn markers_of(review: &Review, path: &str) -> Vec<String> {
    let path = file_path(path);
    review
        .page()
        .rows()
        .iter()
        .filter_map(|row| match row {
            PageRow::PieceMarker(heading) if heading.path == path => {
                Some(heading.piece.to_string())
            }
            _ => None,
        })
        .collect()
}

fn labels_of(review: &Review, path: &str) -> Option<PieceLabels> {
    let path = file_path(path);
    review
        .page()
        .rows()
        .iter()
        .find_map(|row| match row {
            PageRow::PieceHeader(heading) if heading.path == path => Some(heading),
            _ => None,
        })
        .expect("the file has a box")
        .labels
        .as_ref()
        .map(|switch| switch.labels)
}

fn focus_file(review: &mut Review, path: &str) {
    let path = file_path(path);
    for _ in 0..=review.page().changes().len() {
        if review
            .page()
            .focused_change()
            .is_some_and(|change| *change.path() == path)
        {
            return;
        }
        review.focus_next_change();
    }
    panic!("no change of {path} on the screen");
}

#[test]
fn the_rule_hides_labels_when_more_than_half_the_pieces_change_one_or_two_lines() {
    use PieceLabels::{Hidden, Shown};

    assert_eq!(PieceLabels::starting_for_sizes(&[1, 2, 3]), Hidden);
    assert_eq!(PieceLabels::starting_for_sizes(&[1, 1, 1, 9]), Hidden);
    assert_eq!(PieceLabels::starting_for_sizes(&[2]), Hidden);
    assert_eq!(
        PieceLabels::starting_for_sizes(&[1, 3]),
        Shown,
        "exactly half is not most"
    );
    assert_eq!(PieceLabels::starting_for_sizes(&[3, 3, 1]), Shown);
    assert_eq!(PieceLabels::starting_for_sizes(&[3]), Shown);
    assert_eq!(PieceLabels::starting_for_sizes(&[]), Shown);
}

#[test]
fn a_file_of_tiny_pieces_opens_with_no_labels_and_a_file_of_larger_pieces_with_them() {
    let review = Pull::new().open();

    assert_eq!(
        labels_of(&review, "app/Tiny.php"),
        Some(PieceLabels::Hidden)
    );
    assert_eq!(markers_of(&review, "app/Tiny.php"), Vec::<String>::new());
    assert_eq!(
        labels_of(&review, "app/Large.php"),
        Some(PieceLabels::Shown)
    );
    assert_eq!(
        markers_of(&review, "app/Large.php"),
        ["app/Large.php#Cart::x", "app/Large.php#Cart::y"]
    );
}

#[test]
fn a_file_of_one_piece_has_no_labels_to_toggle() {
    let review = Pull::new().open();

    assert_eq!(labels_of(&review, "README.md"), None);
}

#[test]
fn the_header_toggle_changes_one_file_and_leaves_the_others() {
    let mut review = Pull::new().open();

    review
        .display_options_mut()
        .show_piece_labels(&file_path("app/Tiny.php"), PieceLabels::Shown);

    assert_eq!(markers_of(&review, "app/Tiny.php").len(), 3);
    assert_eq!(markers_of(&review, "app/Large.php").len(), 2);

    review
        .display_options_mut()
        .show_piece_labels(&file_path("app/Large.php"), PieceLabels::Hidden);

    assert_eq!(markers_of(&review, "app/Tiny.php").len(), 3);
    assert_eq!(markers_of(&review, "app/Large.php").len(), 0);
}

#[test]
fn hiding_labels_changes_only_the_look() {
    let mut review = Pull::new().open();
    let snapshot = |review: &Review| {
        let pieces: Vec<_> = review
            .current_step()
            .unwrap()
            .pieces
            .iter()
            .map(|piece| (piece.id.to_string(), piece.hunks.len()))
            .collect();
        (
            pieces,
            review.page().lines().count(),
            review.is_ticked(&"cart".into()),
        )
    };
    let before = snapshot(&review);

    review
        .display_options_mut()
        .show_piece_labels(&file_path("app/Large.php"), PieceLabels::Hidden);
    review
        .display_options_mut()
        .show_piece_labels(&file_path("app/Tiny.php"), PieceLabels::Shown);

    assert_eq!(snapshot(&review), before);
}

#[test]
fn l_toggles_the_labels_of_the_focused_changes_file_only() {
    let mut review = Pull::new().open();
    focus_file(&mut review, "app/Tiny.php");

    assert!(review.toggle_focused_piece_labels());
    assert_eq!(labels_of(&review, "app/Tiny.php"), Some(PieceLabels::Shown));
    assert_eq!(
        labels_of(&review, "app/Large.php"),
        Some(PieceLabels::Shown)
    );

    assert!(review.toggle_focused_piece_labels());
    assert_eq!(
        labels_of(&review, "app/Tiny.php"),
        Some(PieceLabels::Hidden)
    );
    assert_eq!(
        review
            .page()
            .focused_change()
            .map(|change| change.path().clone()),
        Some(file_path("app/Tiny.php")),
        "the focus stays where it was"
    );
}

#[test]
fn l_and_r_do_nothing_with_no_focused_change() {
    let mut review = Pull::new().open();
    assert!(review.page().focused_change().is_none());

    assert!(!review.toggle_focused_piece_labels());
    assert!(!review.toggle_focused_file_view());

    assert_eq!(
        labels_of(&review, "app/Tiny.php"),
        Some(PieceLabels::Hidden)
    );
    assert_eq!(
        labels_of(&review, "app/Large.php"),
        Some(PieceLabels::Shown)
    );
}

#[test]
fn l_does_nothing_in_a_file_of_one_piece() {
    let mut review = Pull::new().open();
    focus_file(&mut review, "README.md");

    assert!(!review.toggle_focused_piece_labels());
}

#[test]
fn r_switches_the_focused_changes_markdown_file_to_rendered() {
    let mut review = Pull::new().open();
    focus_file(&mut review, "README.md");

    assert!(review.toggle_focused_file_view());

    let readme = file_path("README.md");
    let view = review
        .page()
        .rows()
        .iter()
        .find_map(|row| match row {
            PageRow::PieceHeader(heading) if heading.path == readme => heading.view.clone(),
            _ => None,
        })
        .expect("the README has a switch");
    assert_eq!(view.view, FileView::Rendered);
}

#[test]
fn r_does_nothing_in_a_file_with_no_rendered_view() {
    let mut review = Pull::new().open();
    focus_file(&mut review, "app/Large.php");

    assert!(!review.toggle_focused_file_view());
}

#[test]
fn a_jump_to_a_piece_whose_label_is_hidden_lands_on_its_file() {
    let review = Pull::new().open();
    let target = review
        .jump_targets("")
        .into_iter()
        .find(|target| {
            matches!(&target.place, guided_review::viewer::JumpPlace::Piece(id)
                if id.to_string() == "app/Tiny.php#Cart::b")
        })
        .expect("the piece is a target");

    let row = review.page().row_of_jump(&target);

    assert!(matches!(
        review.page().row(row),
        Some(PageRow::PieceHeader(heading)) if heading.path == file_path("app/Tiny.php")
    ));
}

#[test]
fn r_and_l_are_page_keys_in_the_view_group_with_defaults_the_sheet_lists() {
    let keys = |command: Command| -> Vec<String> {
        command
            .default_chords()
            .iter()
            .copied()
            .map(Chord::keystroke)
            .collect()
    };

    assert_eq!(keys(Command::ToggleRenderedView), ["r"]);
    assert_eq!(keys(Command::TogglePieceLabels), ["l"]);
    for command in [Command::ToggleRenderedView, Command::TogglePieceLabels] {
        assert_eq!(command.scope(), KeyScope::Page);
        assert_eq!(command.group(), ShortcutGroup::View);
    }
    assert_eq!(
        Command::ToggleRenderedView.config_name(),
        "toggle_rendered_view"
    );
    assert_eq!(
        Command::TogglePieceLabels.config_name(),
        "toggle_piece_labels"
    );
}

#[test]
fn the_palette_offers_the_two_toggles_only_with_a_change_in_focus_that_has_them() {
    let mut review = Pull::new().open();
    let offered = |review: &Review| -> Vec<PaletteAction> {
        review
            .palette_actions(Build::Local)
            .into_iter()
            .filter(|action| {
                matches!(
                    action,
                    PaletteAction::ShowPieceLabels
                        | PaletteAction::HidePieceLabels
                        | PaletteAction::ShowRendered
                        | PaletteAction::ShowCode
                )
            })
            .collect()
    };
    assert_eq!(offered(&review), []);

    focus_file(&mut review, "app/Tiny.php");
    assert_eq!(offered(&review), [PaletteAction::ShowPieceLabels]);

    focus_file(&mut review, "README.md");
    assert_eq!(offered(&review), [PaletteAction::ShowRendered]);
}

#[test]
fn the_keys_and_the_palettes_actions_ask_for_the_same_toggle_of_the_focused_file() {
    let review = Pull::new().open();
    let moment = WindowMoment::default();

    for (intent, toggle) in [
        (Intent::ToggleRenderedView, FileToggle::RenderedView),
        (Intent::TogglePieceLabels, FileToggle::PieceLabels),
    ] {
        assert_eq!(
            review.decide(intent, moment),
            Effect::ToggleFocusedFile(toggle)
        );
    }
    assert_eq!(
        Intent::from(PaletteAction::HidePieceLabels),
        Intent::TogglePieceLabels
    );
    assert_eq!(
        Intent::from(PaletteAction::ShowCode),
        Intent::ToggleRenderedView
    );
}

/// Two methods whose only changes are two re-indented lines each.
fn reindented_cart() -> Review {
    let repo = FixtureRepo::cloned_from("acme/shop");
    let body = |indent: &str| {
        let lines = [
            format!("{indent}$a = 1;"),
            format!("{indent}$b = 2;"),
            "return 0;".to_owned(),
        ];
        let lines: Vec<&str> = lines.iter().map(String::as_str).collect();
        php(&[("x", &lines), ("y", &lines)])
    };
    repo.write("app/Cart.php", &body(""));
    let base = repo.commit("base");
    repo.write("app/Cart.php", &body("  "));
    let head = repo.commit("head");
    Pull { repo, base, head }
        .open_pieces(&[("app/Cart.php", "Cart::x"), ("app/Cart.php", "Cart::y")])
}

#[test]
fn hide_whitespace_does_not_move_the_starting_state_of_the_labels() {
    let mut review = reindented_cart();
    assert_eq!(labels_of(&review, "app/Cart.php"), Some(PieceLabels::Shown));

    review.display_options_mut().toggle_ignore_whitespace();

    assert_eq!(labels_of(&review, "app/Cart.php"), Some(PieceLabels::Shown));
}

#[test]
fn a_reviewers_label_choice_survives_hide_whitespace() {
    let mut review = reindented_cart();
    review
        .display_options_mut()
        .show_piece_labels(&file_path("app/Cart.php"), PieceLabels::Hidden);

    review.display_options_mut().toggle_ignore_whitespace();
    review.display_options_mut().toggle_ignore_whitespace();

    assert_eq!(
        labels_of(&review, "app/Cart.php"),
        Some(PieceLabels::Hidden)
    );
}

#[test]
fn a_rendered_file_keeps_the_focus_so_r_switches_it_back() {
    let mut review = Pull::new().open();
    focus_file(&mut review, "README.md");
    assert!(review.toggle_focused_file_view());

    let focused = review
        .page()
        .focused_change()
        .map(|change| change.path().clone());
    assert_eq!(focused, Some(file_path("README.md")));

    assert!(review.toggle_focused_file_view());
    let view = review
        .page()
        .focused_change()
        .and_then(|change| change.view().map(|switch| switch.view));
    assert_eq!(view, Some(FileView::Code));
}

#[test]
fn tab_moves_on_from_a_rendered_file_and_back_to_it() {
    let mut review = Pull::new().open();
    focus_file(&mut review, "README.md");
    review.toggle_focused_file_view();

    review.focus_next_change();
    let next = review
        .page()
        .focused_change()
        .map(|change| change.path().clone());
    assert_ne!(next, Some(file_path("README.md")));

    review.focus_previous_change();
    let back = review
        .page()
        .focused_change()
        .map(|change| change.path().clone());
    assert_eq!(back, Some(file_path("README.md")));
}
