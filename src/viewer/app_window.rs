use super::active_pr::ActivePr;

/// What the app's one window holds. It owns the window's handle, so whatever asks for the window asks
/// here what to do with it; the handle types are the app's, which keeps this free of GPUI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AppWindow<SettingsOnly, SwitcherOnly, Review> {
    #[default]
    Closed,
    /// No PR is open; the window shows Settings and nothing else.
    SettingsOnly(SettingsOnly),
    /// No PR is open yet, as after a launch from the Dock: the window shows the PR switcher and nothing
    /// else, for the reviewer to pick one.
    SwitcherOnly(SwitcherOnly),
    Review(Review),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShowSwitcher<SettingsOnly, SwitcherOnly, Review> {
    OpenSwitcherOnly,
    /// The window already holds only the switcher: bring it to the front.
    Raise(SwitcherOnly),
    OverReview(Review),
    /// Settings being edited are not thrown away for the switcher; they come to the front instead.
    RaiseSettings(SettingsOnly),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShowSettings<SettingsOnly, SwitcherOnly, Review> {
    OpenSettingsOnly,
    /// The window already holds only Settings: bring it to the front.
    Raise(SettingsOnly),
    OverReview(Review),
    /// Nothing is picked yet, so the window that holds only the switcher holds only Settings instead.
    ReplaceSwitcher(SwitcherOnly),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShowReview<SettingsOnly, SwitcherOnly, Review> {
    OpenWindow,
    /// The Settings being edited stay open, over the review.
    TurnIntoReview(SettingsOnly),
    TurnSwitcherIntoReview(SwitcherOnly),
    /// The PR on screen gives way to the new one.
    Switch(Review),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailedBuild<SettingsOnly, SwitcherOnly, Review> {
    /// Nothing is on screen to keep.
    EndApp,
    /// Settings stays as it is, with the reason above it.
    ShowInSettings(SettingsOnly),
    /// The switcher stays open, with the reason under the query, to pick another PR.
    ShowInSwitcher(SwitcherOnly),
    ShowInReview(Review),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AfterClose {
    EndApp,
    /// The app stays with no window, and the build opens one for its review, or ends the app if it fails.
    WaitForBuild,
}

impl<SettingsOnly: Copy, SwitcherOnly: Copy, Review: Copy>
    AppWindow<SettingsOnly, SwitcherOnly, Review>
{
    pub fn show_switcher(&self) -> ShowSwitcher<SettingsOnly, SwitcherOnly, Review> {
        match *self {
            Self::Closed => ShowSwitcher::OpenSwitcherOnly,
            Self::SwitcherOnly(window) => ShowSwitcher::Raise(window),
            Self::SettingsOnly(window) => ShowSwitcher::RaiseSettings(window),
            Self::Review(window) => ShowSwitcher::OverReview(window),
        }
    }

    pub fn show_settings(&self) -> ShowSettings<SettingsOnly, SwitcherOnly, Review> {
        match *self {
            Self::Closed => ShowSettings::OpenSettingsOnly,
            Self::SwitcherOnly(window) => ShowSettings::ReplaceSwitcher(window),
            Self::SettingsOnly(window) => ShowSettings::Raise(window),
            Self::Review(window) => ShowSettings::OverReview(window),
        }
    }

    pub fn show_review(&self) -> ShowReview<SettingsOnly, SwitcherOnly, Review> {
        match *self {
            Self::Closed => ShowReview::OpenWindow,
            Self::SwitcherOnly(window) => ShowReview::TurnSwitcherIntoReview(window),
            Self::SettingsOnly(window) => ShowReview::TurnIntoReview(window),
            Self::Review(window) => ShowReview::Switch(window),
        }
    }

    pub fn build_failed(&self) -> FailedBuild<SettingsOnly, SwitcherOnly, Review> {
        match *self {
            Self::Closed => FailedBuild::EndApp,
            Self::SwitcherOnly(window) => FailedBuild::ShowInSwitcher(window),
            Self::SettingsOnly(window) => FailedBuild::ShowInSettings(window),
            Self::Review(window) => FailedBuild::ShowInReview(window),
        }
    }

    /// Closing the review ends the app, as before. Closing Settings or the switcher with no PR open
    /// while one builds must not take that build down with it.
    pub fn closed(&mut self, active: &ActivePr) -> AfterClose {
        let was = std::mem::take(self);
        match was {
            Self::SettingsOnly(_) | Self::SwitcherOnly(_) if active.is_building() => {
                AfterClose::WaitForBuild
            }
            Self::Closed | Self::SettingsOnly(_) | Self::SwitcherOnly(_) | Self::Review(_) => {
                AfterClose::EndApp
            }
        }
    }

    /// The window to reach the PR on screen through, when one is.
    pub fn review(&self) -> Option<Review> {
        match *self {
            Self::Review(window) => Some(window),
            Self::Closed | Self::SettingsOnly(_) | Self::SwitcherOnly(_) => None,
        }
    }
}
