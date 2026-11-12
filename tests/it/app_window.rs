use guided_review::viewer::{
    ActivePr, AfterClose, AppWindow, FailedBuild, OpenDecision, OpenOrigin, ShowReview,
    ShowSettings, ShowWriting,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SettingsOnlyWindow;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ReviewWindow;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WritingOnlyWindow;

type Window = AppWindow<SettingsOnlyWindow, WritingOnlyWindow, ReviewWindow>;

#[test]
fn settings_with_no_window_opens_the_window_holding_only_settings() {
    assert_eq!(
        Window::Closed.show_settings(),
        ShowSettings::OpenSettingsOnly
    );
}

#[test]
fn settings_with_a_pr_on_screen_opens_over_that_pr() {
    assert_eq!(
        Window::Review(ReviewWindow).show_settings(),
        ShowSettings::OverReview(ReviewWindow)
    );
}

#[test]
fn settings_asked_again_raises_the_window_already_holding_it() {
    assert_eq!(
        Window::SettingsOnly(SettingsOnlyWindow).show_settings(),
        ShowSettings::Raise(SettingsOnlyWindow)
    );
}

#[test]
fn the_first_review_with_no_window_opens_the_window() {
    assert_eq!(Window::Closed.show_review(), ShowReview::OpenWindow);
}

#[test]
fn a_review_built_while_only_settings_shows_turns_that_window_into_the_review() {
    assert_eq!(
        Window::SettingsOnly(SettingsOnlyWindow).show_review(),
        ShowReview::TurnIntoReview(SettingsOnlyWindow)
    );
}

#[test]
fn a_review_built_while_another_shows_switches_the_window_to_it() {
    assert_eq!(
        Window::Review(ReviewWindow).show_review(),
        ShowReview::Switch(ReviewWindow)
    );
}

#[test]
fn a_failed_build_with_no_window_ends_the_app() {
    assert_eq!(Window::Closed.build_failed(), FailedBuild::EndApp);
}

#[test]
fn a_failed_build_while_only_settings_shows_says_why_in_the_settings_window() {
    assert_eq!(
        Window::SettingsOnly(SettingsOnlyWindow).build_failed(),
        FailedBuild::ShowInSettings(SettingsOnlyWindow)
    );
}

#[test]
fn a_failed_build_while_a_pr_shows_says_why_in_its_window() {
    assert_eq!(
        Window::Review(ReviewWindow).build_failed(),
        FailedBuild::ShowInReview(ReviewWindow)
    );
}

fn building() -> ActivePr {
    let mut active = ActivePr::default();
    let pr = "https://github.com/acme/shop/pull/7".parse().unwrap();
    let OpenDecision::Build(_) = active.request(pr, OpenOrigin::Reviewer) else {
        panic!("the first open of a PR builds it");
    };
    active
}

#[test]
fn closing_settings_with_no_pr_open_and_nothing_building_ends_the_app() {
    let mut window = Window::SettingsOnly(SettingsOnlyWindow);

    assert_eq!(window.closed(&ActivePr::default()), AfterClose::EndApp);
    assert_eq!(window, Window::Closed);
}

#[test]
fn closing_settings_while_a_pr_builds_keeps_the_app_for_the_review() {
    let mut window = Window::SettingsOnly(SettingsOnlyWindow);

    assert_eq!(window.closed(&building()), AfterClose::WaitForBuild);
    assert_eq!(window, Window::Closed);
    assert_eq!(window.show_review(), ShowReview::OpenWindow);
}

#[test]
fn closing_the_review_ends_the_app_even_while_another_pr_builds() {
    let mut window = Window::Review(ReviewWindow);

    assert_eq!(window.closed(&building()), AfterClose::EndApp);
}

#[test]
fn only_a_window_holding_a_review_has_a_pr_to_reach() {
    assert_eq!(Window::Closed.review(), None);
    assert_eq!(Window::SettingsOnly(SettingsOnlyWindow).review(), None);
    assert_eq!(Window::Review(ReviewWindow).review(), Some(ReviewWindow));
}

#[test]
fn a_tour_being_written_with_no_window_opens_the_window_holding_only_its_loading_screen() {
    assert_eq!(Window::Closed.show_writing(), ShowWriting::OpenWindow);
}

#[test]
fn a_tour_being_written_while_a_pr_shows_goes_over_that_pr() {
    assert_eq!(
        Window::Review(ReviewWindow).show_writing(),
        ShowWriting::OverReview(ReviewWindow)
    );
}

#[test]
fn a_tour_being_written_asked_again_raises_the_window_already_showing_it() {
    assert_eq!(
        Window::WritingOnly(WritingOnlyWindow).show_writing(),
        ShowWriting::Raise(WritingOnlyWindow)
    );
}

#[test]
fn a_tour_being_written_while_only_settings_shows_leaves_the_settings_window_alone() {
    assert_eq!(
        Window::SettingsOnly(SettingsOnlyWindow).show_writing(),
        ShowWriting::LeaveSettings
    );
}

#[test]
fn a_review_built_while_only_the_loading_screen_shows_turns_that_window_into_the_review() {
    assert_eq!(
        Window::WritingOnly(WritingOnlyWindow).show_review(),
        ShowReview::TurnWritingIntoReview(WritingOnlyWindow)
    );
}

#[test]
fn a_failed_build_while_only_the_loading_screen_shows_ends_the_app() {
    assert_eq!(
        Window::WritingOnly(WritingOnlyWindow).build_failed(),
        FailedBuild::EndApp
    );
}

#[test]
fn closing_the_loading_screen_ends_the_app_unless_the_written_tour_is_building() {
    let mut window = Window::WritingOnly(WritingOnlyWindow);
    assert_eq!(window.closed(&ActivePr::default()), AfterClose::EndApp);

    let mut window = Window::WritingOnly(WritingOnlyWindow);
    assert_eq!(window.closed(&building()), AfterClose::WaitForBuild);
}

#[test]
fn the_loading_screen_window_has_no_pr_to_reach() {
    assert_eq!(Window::WritingOnly(WritingOnlyWindow).review(), None);
}

#[test]
fn settings_asked_while_only_the_loading_screen_shows_wait_for_the_review() {
    assert_eq!(
        Window::WritingOnly(WritingOnlyWindow).show_settings(),
        ShowSettings::NotWhileWriting
    );
}
