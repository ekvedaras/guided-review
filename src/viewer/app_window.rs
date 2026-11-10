use super::active_pr::ActivePr;

/// What the app's one window holds. It owns the window's handle, so whatever asks for the window asks
/// here what to do with it; the handle types are the app's, which keeps this free of GPUI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AppWindow<SettingsOnly, Review> {
    #[default]
    Closed,
    /// No PR is open; the window shows Settings and nothing else.
    SettingsOnly(SettingsOnly),
    Review(Review),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShowSettings<SettingsOnly, Review> {
    OpenSettingsOnly,
    /// The window already holds only Settings: bring it to the front.
    Raise(SettingsOnly),
    OverReview(Review),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShowReview<SettingsOnly, Review> {
    OpenWindow,
    /// The Settings being edited stay open, over the review.
    TurnIntoReview(SettingsOnly),
    /// The PR on screen gives way to the new one.
    Switch(Review),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailedBuild<SettingsOnly, Review> {
    /// Nothing is on screen to keep.
    EndApp,
    /// Settings stays as it is, with the reason above it.
    ShowInSettings(SettingsOnly),
    ShowInReview(Review),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AfterClose {
    EndApp,
    /// The app stays with no window, and the build opens one for its review, or ends the app if it fails.
    WaitForBuild,
}

impl<SettingsOnly: Copy, Review: Copy> AppWindow<SettingsOnly, Review> {
    pub fn show_settings(&self) -> ShowSettings<SettingsOnly, Review> {
        match *self {
            Self::Closed => ShowSettings::OpenSettingsOnly,
            Self::SettingsOnly(window) => ShowSettings::Raise(window),
            Self::Review(window) => ShowSettings::OverReview(window),
        }
    }

    pub fn show_review(&self) -> ShowReview<SettingsOnly, Review> {
        match *self {
            Self::Closed => ShowReview::OpenWindow,
            Self::SettingsOnly(window) => ShowReview::TurnIntoReview(window),
            Self::Review(window) => ShowReview::Switch(window),
        }
    }

    pub fn build_failed(&self) -> FailedBuild<SettingsOnly, Review> {
        match *self {
            Self::Closed => FailedBuild::EndApp,
            Self::SettingsOnly(window) => FailedBuild::ShowInSettings(window),
            Self::Review(window) => FailedBuild::ShowInReview(window),
        }
    }

    /// Closing the review ends the app, as before. Closing Settings with no PR open while one builds
    /// must not take that build down with it.
    pub fn closed(&mut self, active: &ActivePr) -> AfterClose {
        let was = std::mem::take(self);
        match was {
            Self::SettingsOnly(_) if active.is_building() => AfterClose::WaitForBuild,
            Self::Closed | Self::SettingsOnly(_) | Self::Review(_) => AfterClose::EndApp,
        }
    }

    /// The window to reach the PR on screen through, when one is.
    pub fn review(&self) -> Option<Review> {
        match *self {
            Self::Review(window) => Some(window),
            Self::Closed | Self::SettingsOnly(_) => None,
        }
    }
}
