use guided_review::viewer::{AppWindow, FailedBuild, ShowReview, ShowSettings};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SettingsOnlyWindow;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ReviewWindow;

type Window = AppWindow<SettingsOnlyWindow, ReviewWindow>;

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
fn a_failed_build_while_only_settings_shows_keeps_settings_open() {
    assert_eq!(
        Window::SettingsOnly(SettingsOnlyWindow).build_failed(),
        FailedBuild::KeepSettings
    );
}

#[test]
fn a_failed_build_while_a_pr_shows_says_why_in_its_window() {
    assert_eq!(
        Window::Review(ReviewWindow).build_failed(),
        FailedBuild::ShowIn(ReviewWindow)
    );
}

#[test]
fn only_a_window_holding_a_review_has_a_pr_to_reach() {
    assert_eq!(Window::Closed.review(), None);
    assert_eq!(Window::SettingsOnly(SettingsOnlyWindow).review(), None);
    assert_eq!(Window::Review(ReviewWindow).review(), Some(ReviewWindow));
}
