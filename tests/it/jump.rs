use crate::support::FixtureRepo;
use guided_review::file_path::FilePath;
use guided_review::local_git::LocalGit;
use guided_review::pieces::PieceId;
use guided_review::pull_request::RepoName;
use guided_review::tour::Tour;
use guided_review::update::Build;
use guided_review::viewer::{Command, JumpPlace, PaletteEntry, Review, ReviewStateFile, Screen};
use serde_json::{Value, json};

fn open(steps: Vec<Value>) -> Review {
    let repo = FixtureRepo::cloned_from("acme/shop");
    repo.write(
        "app/Order.php",
        "<?php\n\nclass Order\n{\n    public function total(): int\n    {\n        return 1;\n    }\n}\n",
    );
    repo.write("README.md", "hello\n");
    let base = repo.commit("base");
    repo.write(
        "app/Order.php",
        "<?php\n\nclass Order\n{\n    public function total(): int\n    {\n        return 2;\n    }\n}\n",
    );
    repo.write("README.md", "hello\nworld\n");
    let head = repo.commit("head");
    let tour: Tour = serde_json::from_value(json!({
        "pr": "https://github.com/acme/shop/pull/7",
        "base": base,
        "head": head,
        "title": "Orders cost two",
        "summary": [],
        "steps": steps,
    }))
    .unwrap();
    let source = LocalGit::find(&RepoName::new("acme", "shop"), repo.path())
        .unwrap()
        .unwrap();
    let state_file = ReviewStateFile::for_pr(
        repo.home(),
        &"https://github.com/acme/shop/pull/7".parse().unwrap(),
    );
    Review::open(tour, &source, state_file).unwrap()
}

fn step(id: &str, title: &str, pieces: Value) -> Value {
    json!({"id": id, "title": title, "why": "because", "pieces": pieces})
}

fn shop_tour() -> Vec<Value> {
    vec![
        step(
            "totals",
            "Order totals go up",
            json!([{"file": "app/Order.php", "symbol": "Order::total"}]),
        ),
        step("docs", "Readme says hello", json!([{"file": "README.md"}])),
    ]
}

fn kind(place: &JumpPlace) -> &'static str {
    match place {
        JumpPlace::StepTop => "Step",
        JumpPlace::File(_) => "File",
        JumpPlace::Piece(_) => "Symbol",
    }
}

fn labels(review: &Review, query: &str) -> Vec<String> {
    review
        .jump_targets(query)
        .iter()
        .map(|target| format!("{} {}", kind(&target.place), target.label()))
        .collect()
}

#[test]
fn an_empty_query_lists_steps_files_and_symbols_in_reading_order() {
    let review = open(shop_tour());

    assert_eq!(
        labels(&review, ""),
        [
            "Step Order totals go up",
            "File app/Order.php",
            "Symbol app/Order.php#Order::total",
            "Step Readme says hello",
            "File README.md",
        ]
    );
}

#[test]
fn a_query_matches_letters_in_order_ignoring_case() {
    let review = open(shop_tour());

    assert_eq!(
        labels(&review, "ordtot"),
        [
            "Step Order totals go up",
            "Symbol app/Order.php#Order::total"
        ]
    );
    assert_eq!(
        labels(&review, "README"),
        ["File README.md", "Step Readme says hello"]
    );
    assert_eq!(labels(&review, "zzz"), [] as [String; 0]);
}

#[test]
fn letters_scattered_inside_words_match_nothing() {
    let review = open(shop_tour());

    assert_eq!(labels(&review, "rdtl"), [] as [String; 0]);
    assert_eq!(
        labels(&review, "otals"),
        ["Step Order totals go up"],
        "a run found whole in the text matches anywhere"
    );
}

#[test]
fn the_closest_match_comes_first() {
    let review = open(shop_tour());

    assert_eq!(labels(&review, "total")[0], "Step Order totals go up");
    assert_eq!(
        labels(&review, "Order::total")[0],
        "Symbol app/Order.php#Order::total"
    );
}

#[test]
fn picking_a_target_opens_the_step_that_holds_it() {
    let mut review = open(shop_tour());

    let file = review
        .jump_targets("README")
        .into_iter()
        .find(|target| matches!(target.place, JumpPlace::File(_)))
        .unwrap();
    review.jump_to(&file);

    assert_eq!(review.screen(), &Screen::Step("docs".into()));
}

#[test]
fn a_symbol_target_names_its_file_and_step() {
    let review = open(shop_tour());

    let symbol = review
        .jump_targets("Order::total")
        .into_iter()
        .find(|target| matches!(target.place, JumpPlace::Piece(_)))
        .unwrap();

    assert_eq!(symbol.screen, Screen::Step("totals".into()));
    assert_eq!(symbol.detail(), Some("Order totals go up"));
}

#[test]
fn picking_a_target_in_a_folded_step_unfolds_it_for_the_session() {
    let mut review = open(shop_tour());
    let skim = "totals".into();
    review.toggle_fold(&skim);
    assert!(review.is_folded(&skim));

    let symbol = review.jump_targets("Order::total")[0].clone();
    review.jump_to(&symbol);

    assert!(!review.is_folded(&skim));
}

#[test]
fn picking_a_target_in_an_open_step_leaves_it_open() {
    let mut review = open(shop_tour());
    let id = "totals".into();

    let symbol = review.jump_targets("Order::total")[0].clone();
    review.jump_to(&symbol);

    assert!(!review.is_folded(&id));
}

#[test]
fn files_only_the_safety_net_shows_are_jump_targets() {
    let mut review = open(vec![step(
        "totals",
        "Order totals go up",
        json!([{"file": "app/Order.php", "symbol": "Order::total"}]),
    )]);

    let readme = review
        .jump_targets("README")
        .into_iter()
        .find(|target| matches!(target.place, JumpPlace::File(_)))
        .unwrap();
    assert_eq!(readme.detail(), Some("Safety net"));

    review.jump_to(&readme);
    assert_eq!(review.screen(), &Screen::SafetyNet);
}

#[test]
fn a_target_says_where_it_lands_whatever_its_label_says() {
    let review = open(shop_tour());
    let path = FilePath::new("app/Order.php").unwrap();

    let places: Vec<JumpPlace> = review
        .jump_targets("")
        .into_iter()
        .map(|target| target.place)
        .collect();

    assert_eq!(
        places,
        [
            JumpPlace::StepTop,
            JumpPlace::File(path.clone()),
            JumpPlace::Piece(PieceId::symbol(path, "Order::total").unwrap()),
            JumpPlace::StepTop,
            JumpPlace::File(FilePath::new("README.md").unwrap()),
        ]
    );
}

fn entry_name(entry: &PaletteEntry) -> String {
    match entry {
        PaletteEntry::Place(target) => target.label().into_owned(),
        PaletteEntry::Action(action) => action.label().to_owned(),
    }
}

/// Each group as `Heading: entry, entry`.
fn palette(review: &Review, query: &str) -> Vec<String> {
    review
        .palette(query, Build::Local)
        .iter()
        .map(|group| {
            let entries: Vec<String> = group.entries.iter().map(entry_name).collect();
            format!("{}: {}", group.group.title(), entries.join(", "))
        })
        .collect()
}

fn actions(review: &Review) -> Vec<&'static str> {
    actions_on(review, Build::Local)
}

fn actions_on(review: &Review, build: Build) -> Vec<&'static str> {
    review
        .palette_actions(build)
        .iter()
        .map(|action| action.label())
        .collect()
}

#[test]
fn an_empty_query_groups_steps_files_and_symbols_in_reading_order_then_actions() {
    let review = open(shop_tour());

    let groups = palette(&review, "");

    assert_eq!(
        groups[..3],
        [
            "Steps: Order totals go up, Readme says hello",
            "Files: app/Order.php, README.md",
            "Symbols: app/Order.php#Order::total",
        ]
    );
    assert!(groups[3].starts_with("Actions: "), "{groups:?}");
}

#[test]
fn the_group_holding_the_best_match_comes_first_and_actions_come_after_places() {
    let review = open(shop_tour());

    assert_eq!(
        palette(&review, "README"),
        ["Files: README.md", "Steps: Readme says hello"]
    );
    assert_eq!(
        palette(&review, "set"),
        ["Actions: Settings"],
        "actions match their names too"
    );
}

#[test]
fn a_query_that_starts_with_a_chevron_lists_only_actions() {
    let review = open(shop_tour());

    let only_actions = palette(&review, ">");
    assert_eq!(only_actions.len(), 1);
    assert!(only_actions[0].starts_with("Actions: "));
    assert_eq!(palette(&review, "> readme"), [] as [String; 0]);
    assert_eq!(palette(&review, ">sett"), ["Actions: Settings"]);
}

#[test]
fn the_actions_follow_what_the_open_screen_allows() {
    let mut review = open(shop_tour());

    let on_summary = actions(&review);
    assert!(on_summary.contains(&"Next unread step"));
    assert!(on_summary.contains(&"Settings"));
    assert!(on_summary.contains(&"Keyboard shortcuts"));
    assert!(on_summary.contains(&"Switch pull request"));
    assert!(on_summary.contains(&"Open on GitHub"));
    assert!(!on_summary.contains(&"Mark step as read"));
    assert!(!on_summary.contains(&"Ask about this step"));

    review.go_to(&"totals".into());
    let on_step = actions(&review);
    assert!(on_step.contains(&"Mark step as read"));
    assert!(on_step.contains(&"Ask about this step"));
    assert!(on_step.contains(&"Hide whitespace"));
    assert!(on_step.contains(&"Wrap lines"));

    review.tick(&"totals".into()).unwrap();
    review.display_options_mut().toggle_ignore_whitespace();
    review.display_options_mut().toggle_wrap_lines();
    let flipped = actions(&review);
    assert!(flipped.contains(&"Mark step as unread"));
    assert!(flipped.contains(&"Show whitespace"));
    assert!(flipped.contains(&"Stop wrapping lines"));
}

#[test]
fn finishing_the_review_is_an_action_only_where_the_pr_allows_it() {
    let mut review = open(shop_tour());
    review.viewer_authorship_is(false);
    assert_eq!(
        review.can_finish_review(),
        actions(&review).contains(&"Finish review")
    );
    review.viewer_authorship_is(true);
    assert_eq!(
        review.can_finish_review(),
        actions(&review).contains(&"Finish review")
    );
}

#[test]
fn an_action_names_the_shortcut_that_does_it() {
    let review = open(shop_tour());

    let settings = review
        .palette_actions(Build::Local)
        .into_iter()
        .find(|action| action.label() == "Settings")
        .unwrap();
    assert_eq!(settings.command(), Some(Command::Settings));
}

#[test]
fn the_whitespace_and_ask_actions_name_their_shortcuts() {
    use guided_review::viewer::PaletteAction;

    assert_eq!(
        PaletteAction::HideWhitespace.command(),
        Some(Command::HideWhitespace)
    );
    assert_eq!(
        PaletteAction::ShowWhitespace.command(),
        Some(Command::HideWhitespace)
    );
    assert_eq!(
        PaletteAction::AskAboutStep.command(),
        Some(Command::AskAboutStep)
    );
}

#[test]
fn the_sidebar_flags_steps_with_something_to_check_and_marks_skim_steps() {
    let review = open(vec![
        json!({"id": "totals", "title": "Totals", "why": "w", "check": "Rounding", "pieces": [{"file": "app/Order.php"}]}),
        json!({"id": "docs", "title": "Docs", "why": "w", "skim": true, "pieces": [{"file": "README.md"}]}),
    ]);

    let entries = review.sidebar();
    assert!(entries[0].has_check);
    assert!(!entries[0].is_skim);
    assert!(!entries[1].has_check);
    assert!(entries[1].is_skim);
}

#[test]
fn the_palette_offers_a_restart_that_comes_back_to_this_screen() {
    let review = open(shop_tour());

    assert_eq!(palette(&review, ">restart"), ["Actions: Restart app here"]);
}

#[test]
fn only_the_release_build_can_check_for_updates() {
    let review = open(shop_tour());

    assert!(actions_on(&review, Build::Release).contains(&"Check for updates"));
    assert!(!actions_on(&review, Build::Local).contains(&"Check for updates"));
    assert!(!actions_on(&review, Build::Test).contains(&"Check for updates"));
}
