//! The key for Open all pieces / Fold all pieces: the toolbar button's job, on the keyboard.

use crate::support::{FixtureRepo, file_path};
use guided_review::build::Build;
use guided_review::local_git::LocalGit;
use guided_review::pull_request::{RepoName, Sha};
use guided_review::tour::{BasicTour, GuidedTour, TourPin};
use guided_review::viewer::{
    Command, Effect, Intent, PaletteAction, Review, ReviewStateFile, Screen, WindowMoment,
};
use serde_json::json;

const PR: &str = "https://github.com/acme/shop/pull/7";

fn two_file_pull() -> (FixtureRepo, String, String) {
    let repo = FixtureRepo::cloned_from("acme/shop");
    repo.write("a.txt", "one\n");
    repo.write("b.txt", "one\n");
    let base = repo.commit("base");
    repo.write("a.txt", "two\n");
    repo.write("b.txt", "two\n");
    let head = repo.commit("head");
    (repo, base, head)
}

/// A skim step with a piece in each file, then a plain step that does not fold.
fn guided() -> (FixtureRepo, Review) {
    let (repo, base, head) = two_file_pull();
    let tour: GuidedTour = serde_json::from_value(json!({
        "pr": PR, "base": base, "head": head, "title": "Two files", "summary": [],
        "steps": [
            {"id": "skim", "title": "Skim", "why": "because", "skim": true,
             "pieces": [{"file": "a.txt"}, {"file": "b.txt"}], "children": []},
            {"id": "plain", "title": "Plain", "why": "because",
             "pieces": [{"file": "a.txt"}], "children": []},
        ],
    }))
    .unwrap();
    let source = LocalGit::find(&RepoName::new("acme", "shop"), repo.path())
        .unwrap()
        .unwrap();
    let state_file = ReviewStateFile::for_subject(&repo.folders(), &PR.parse().unwrap());
    let mut review = Review::open(
        tour,
        &source,
        state_file,
        crate::support::fake_ports::every_capability(),
    )
    .unwrap();
    review.go_to(&"skim".into());
    (repo, review)
}

fn basic() -> (FixtureRepo, Review) {
    let (repo, base, head) = two_file_pull();
    let tour = BasicTour::new(
        TourPin {
            version: 1,
            pr: PR.to_owned(),
            local: None,
            repo: None,
            host: None,
            base: Sha::new(&base),
            head: Sha::new(&head),
            previous_head: None,
        },
        "Two files".to_owned(),
    );
    let source = LocalGit::find(&RepoName::new("acme", "shop"), repo.path())
        .unwrap()
        .unwrap();
    let state_file = ReviewStateFile::for_subject(&repo.folders(), &PR.parse().unwrap());
    let mut review = Review::open_basic(
        tour,
        &source,
        state_file,
        crate::support::fake_ports::every_capability(),
    )
    .unwrap();
    review.go_to_file(&file_path("a.txt"));
    (repo, review)
}

fn skim() -> Screen {
    Screen::Step("skim".into())
}

fn offered(review: &Review) -> Vec<&'static str> {
    review
        .palette_actions(Build::Local)
        .iter()
        .map(|action| action.label())
        .filter(|label| label.ends_with("all pieces"))
        .collect()
}

#[test]
fn the_key_is_o_in_the_page_scope_under_its_own_config_name() {
    let chords = Command::ToggleFold.default_chords();
    assert_eq!(chords.len(), 1);
    assert_eq!(chords[0].written(), "o");
    assert_eq!(Command::ToggleFold.config_name(), "toggle_fold");
    assert_eq!(
        Command::of_config_name("toggle_fold"),
        Some(Command::ToggleFold)
    );
}

#[test]
fn the_key_opens_every_box_of_a_folded_step_and_pressed_again_folds_them() {
    let (_repo, mut review) = guided();
    assert!(review.is_box_folded(&skim(), &file_path("a.txt")));

    assert!(review.toggle_screen_fold());
    assert!(!review.is_box_folded(&skim(), &file_path("a.txt")));
    assert!(!review.is_box_folded(&skim(), &file_path("b.txt")));

    assert!(review.toggle_screen_fold());
    assert!(review.is_box_folded(&skim(), &file_path("a.txt")));
    assert!(review.is_box_folded(&skim(), &file_path("b.txt")));
}

#[test]
fn the_key_does_nothing_on_a_step_that_does_not_fold() {
    let (_repo, mut review) = guided();
    review.go_to(&"plain".into());

    assert!(!review.toggle_screen_fold());
    assert_eq!(offered(&review), Vec::<&str>::new());
}

#[test]
fn the_palette_offers_open_then_fold_where_the_toolbar_has_the_toggle() {
    let (_repo, mut review) = guided();
    assert_eq!(offered(&review), ["Open all pieces"]);

    review.toggle_screen_fold();
    assert_eq!(offered(&review), ["Fold all pieces"]);
}

#[test]
fn the_key_and_the_palette_ask_for_the_same_thing() {
    let (_repo, review) = guided();

    assert_eq!(
        review.decide(Intent::ToggleFold, WindowMoment::default()),
        Effect::ToggleScreenFold
    );
    assert_eq!(
        Intent::from(PaletteAction::OpenAllPieces),
        Intent::ToggleFold
    );
    assert_eq!(
        Intent::from(PaletteAction::FoldAllPieces),
        Intent::ToggleFold
    );
    assert_eq!(
        PaletteAction::OpenAllPieces.command(),
        Some(Command::ToggleFold)
    );
}

#[test]
fn on_a_basic_tours_file_screen_the_key_folds_the_box_and_opens_it_again() {
    let (_repo, mut review) = basic();
    let screen = Screen::File(file_path("a.txt"));
    assert_eq!(offered(&review), ["Fold all pieces"]);

    assert!(review.toggle_screen_fold());
    assert!(review.is_box_folded(&screen, &file_path("a.txt")));
    assert_eq!(offered(&review), ["Open all pieces"]);

    assert!(review.toggle_screen_fold());
    assert!(!review.is_box_folded(&screen, &file_path("a.txt")));
}
