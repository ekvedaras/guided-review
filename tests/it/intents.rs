use crate::support::FixtureRepo;
use guided_review::local_git::LocalGit;
use guided_review::pull_request::RepoName;
use guided_review::tour::Tour;
use guided_review::viewer::{
    Effect, Intent, Overlay, PaletteAction, Refusal, Reopening, Review, ReviewStateFile, Screen,
    SettingsSheet, WindowMoment,
};
use serde_json::json;

fn three_step_review(repo: &FixtureRepo) -> Review {
    repo.write("README.md", "hello\n");
    let base = repo.commit("base");
    repo.write("README.md", "hello\nworld\n");
    let head = repo.commit("head");
    let tour: Tour = serde_json::from_value(json!({
        "pr": "https://github.com/acme/shop/pull/7",
        "base": base,
        "head": head,
        "title": "Hello world",
        "steps": [
            {"id": "one", "title": "One"},
            {"id": "two", "title": "Two"},
            {"id": "three", "title": "Three"},
        ],
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

fn act(review: &mut Review, intent: Intent) {
    match review.decide(intent, WindowMoment::default()) {
        Effect::Change(change) => review.apply(change).unwrap(),
        other => panic!("{intent:?} changed nothing: {other:?}"),
    }
}

fn on_step(review: &mut Review, id: &str) {
    review.go_to(&id.into());
}

#[test]
fn the_forward_key_does_not_tick_a_step_whose_end_was_not_seen() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    let mut review = three_step_review(&repo);
    on_step(&mut review, "one");

    act(&mut review, Intent::NextScreen);

    assert!(!review.is_ticked(&"one".into()));
    assert_eq!(review.screen(), &Screen::Step("two".into()));
}

#[test]
fn the_forward_key_ticks_a_step_whose_end_was_seen() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    let mut review = three_step_review(&repo);
    on_step(&mut review, "one");

    review.saw_end_of_screen();
    act(&mut review, Intent::NextScreen);

    assert!(review.is_ticked(&"one".into()));
    assert_eq!(review.screen(), &Screen::Step("two".into()));
}

#[test]
fn an_end_seen_on_another_screen_does_not_let_the_forward_key_tick() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    let mut review = three_step_review(&repo);
    on_step(&mut review, "one");
    review.saw_end_of_screen();
    on_step(&mut review, "two");

    act(&mut review, Intent::NextScreen);

    assert!(!review.is_ticked(&"two".into()));
}

#[test]
fn the_footers_mark_as_read_ticks_without_the_end_seen() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    let mut review = three_step_review(&repo);
    on_step(&mut review, "one");

    act(&mut review, Intent::MarkReadAndGoOn);

    assert!(
        review.is_ticked(&"one".into()),
        "unlike the forward key, the footer's button does not ask whether the end was seen"
    );
    assert_eq!(review.screen(), &Screen::Step("two".into()));
}

#[test]
fn going_on_from_a_step_read_already_keeps_its_tick() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    let mut review = three_step_review(&repo);
    on_step(&mut review, "one");
    review.tick(&"one".into()).unwrap();

    review.saw_end_of_screen();
    act(&mut review, Intent::NextScreen);
    on_step(&mut review, "one");
    act(&mut review, Intent::MarkReadAndGoOn);

    assert!(review.is_ticked(&"one".into()));
}

const REOPENING: WindowMoment = WindowMoment {
    reopening: Reopening::Underway,
    settings: SettingsSheet::Closed,
};

const SETTINGS_OPEN: WindowMoment = WindowMoment {
    reopening: Reopening::NotUnderway,
    settings: SettingsSheet::Open,
};

#[test]
fn no_command_acts_while_reopening() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    let mut review = three_step_review(&repo);
    on_step(&mut review, "one");

    for intent in [
        Intent::NextScreen,
        Intent::PreviousScreen,
        Intent::NextUnreadStep,
        Intent::ToggleRead,
        Intent::ToggleWrapLines,
        Intent::ToggleJump,
        Intent::ToggleSettings,
        Intent::ShowSettings,
        Intent::ToggleShortcuts,
    ] {
        assert_eq!(
            review.decide(intent, REOPENING),
            Effect::Refused(Refusal::Reopening),
            "{intent:?}"
        );
    }
}

#[test]
fn settings_open_blocks_the_jump_palette_and_the_pr_switcher() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    let review = three_step_review(&repo);

    for intent in [Intent::ToggleJump, Intent::TogglePrSwitcher] {
        assert_eq!(
            review.decide(intent, SETTINGS_OPEN),
            Effect::Refused(Refusal::SettingsOpen),
            "{intent:?}"
        );
    }
    assert_eq!(
        review.decide(Intent::ToggleJump, WindowMoment::default()),
        Effect::Toggle(Overlay::Jump)
    );
    assert_eq!(
        review.decide(Intent::TogglePrSwitcher, WindowMoment::default()),
        Effect::Toggle(Overlay::PrSwitcher)
    );
}

#[test]
fn settings_asked_for_from_outside_open_it_and_never_close_it() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    let review = three_step_review(&repo);

    assert_eq!(
        review.decide(Intent::ShowSettings, WindowMoment::default()),
        Effect::OpenSettings
    );
    assert_eq!(
        review.decide(Intent::ShowSettings, SETTINGS_OPEN),
        Effect::Nothing
    );
    assert_eq!(
        review.decide(Intent::ToggleSettings, SETTINGS_OPEN),
        Effect::Toggle(Overlay::Settings)
    );
}

#[test]
fn the_palettes_mark_as_read_ticks_and_mark_as_unread_unticks() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    let mut review = three_step_review(&repo);
    on_step(&mut review, "one");

    act(&mut review, PaletteAction::MarkStepRead.into());
    assert!(review.is_ticked(&"one".into()));
    assert_eq!(review.screen(), &Screen::Step("one".into()), "it stays");

    act(&mut review, PaletteAction::MarkStepUnread.into());
    assert!(!review.is_ticked(&"one".into()));
}

#[test]
fn marking_as_read_on_the_summary_does_nothing() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    let review = three_step_review(&repo);

    assert_eq!(
        review.decide(Intent::ToggleRead, WindowMoment::default()),
        Effect::Nothing
    );
}

#[test]
fn the_palettes_whitespace_and_wrap_actions_flip_the_display_options() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    let mut review = three_step_review(&repo);
    let (wraps, ignores) = (
        review.display_options().wraps_lines(),
        review.display_options().ignores_whitespace(),
    );

    act(&mut review, PaletteAction::WrapLines.into());
    act(&mut review, PaletteAction::HideWhitespace.into());

    assert_eq!(review.display_options().wraps_lines(), !wraps);
    assert_eq!(review.display_options().ignores_whitespace(), !ignores);
}

#[test]
fn restart_opens_the_pr_at_the_step_on_screen() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    let mut review = three_step_review(&repo);
    on_step(&mut review, "two");

    let Effect::Restart(restart) = review.decide(Intent::RestartApp, WindowMoment::default())
    else {
        panic!("no restart");
    };

    assert_eq!(
        restart.args(),
        [
            "restart",
            "https://github.com/acme/shop/pull/7",
            "--at",
            "two"
        ]
    );
}

#[test]
fn restart_on_the_summary_opens_the_pr_where_it_starts() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    let review = three_step_review(&repo);

    assert_eq!(
        review.restart_args().args(),
        ["restart", "https://github.com/acme/shop/pull/7"]
    );
}

#[test]
fn the_scroll_script_goes_forward_with_the_next_step_opened() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    let mut review = three_step_review(&repo);
    review.tick(&"two".into()).unwrap();
    on_step(&mut review, "one");
    assert!(review.is_folded(&"two".into()), "a read step folds");

    assert!(review.go_forward_unfolded());

    assert_eq!(review.screen(), &Screen::Step("two".into()));
    assert!(!review.is_folded(&"two".into()));
}
