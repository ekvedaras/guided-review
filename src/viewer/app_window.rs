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
pub enum FailedBuild<Review> {
    /// Nothing is on screen to keep.
    EndApp,
    /// The reason goes to the app's log; Settings stays as it is.
    KeepSettings,
    ShowIn(Review),
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

    pub fn build_failed(&self) -> FailedBuild<Review> {
        match *self {
            Self::Closed => FailedBuild::EndApp,
            Self::SettingsOnly(_) => FailedBuild::KeepSettings,
            Self::Review(window) => FailedBuild::ShowIn(window),
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
