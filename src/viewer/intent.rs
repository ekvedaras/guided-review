//! What the reader asks of the review's window, by key, button or palette, and what that does.
//!
//! The window reports what it alone knows (an overlay open, a reopen under way, the end of the
//! screen on show) and carries out the `Effect`; the rules that pick the effect are here, once.

use super::jump::PaletteAction;
use super::review::{ReviewCore, Screen};
use crate::pull_request::PullRequestUrl;
use crate::tour::StepId;

/// One thing the reader asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Intent {
    /// The forward key: on, ticking the step it leaves only once its end was seen.
    NextScreen,
    /// The footer's main button: ticks the step it leaves, then goes on.
    MarkReadAndGoOn,
    PreviousScreen,
    NextUnreadStep,
    /// Ticks or unticks the step on screen.
    ToggleRead,
    ToggleWrapLines,
    ToggleWhitespace,
    /// `R`, on the focused change's file.
    ToggleRenderedView,
    /// `L`, on the focused change's file.
    TogglePieceLabels,
    ToggleJump,
    TogglePrSwitcher,
    /// The PR switcher asked for from outside the window, by a launch of Guided Review.app.
    ShowPrSwitcher,
    ToggleSettings,
    /// Settings asked for from outside the window, by `guided-review settings`.
    ShowSettings,
    ToggleShortcuts,
    AskAboutScreen,
    FinishReview,
    OpenOnGitHub,
    CheckForUpdates,
    RestartApp,
}

/// A palette action does what its button or key does.
impl From<PaletteAction> for Intent {
    fn from(action: PaletteAction) -> Self {
        match action {
            PaletteAction::NextUnreadStep => Self::NextUnreadStep,
            PaletteAction::MarkStepRead | PaletteAction::MarkStepUnread => Self::ToggleRead,
            PaletteAction::WrapLines | PaletteAction::StopWrappingLines => Self::ToggleWrapLines,
            PaletteAction::HideWhitespace | PaletteAction::ShowWhitespace => Self::ToggleWhitespace,
            PaletteAction::ShowPieceLabels | PaletteAction::HidePieceLabels => {
                Self::TogglePieceLabels
            }
            PaletteAction::ShowRendered | PaletteAction::ShowCode => Self::ToggleRenderedView,
            PaletteAction::AskAboutStep => Self::AskAboutScreen,
            PaletteAction::FinishReview => Self::FinishReview,
            PaletteAction::OpenOnGitHub => Self::OpenOnGitHub,
            PaletteAction::SwitchPr => Self::TogglePrSwitcher,
            PaletteAction::Settings => Self::ToggleSettings,
            PaletteAction::Shortcuts => Self::ToggleShortcuts,
            PaletteAction::CheckForUpdates => Self::CheckForUpdates,
            PaletteAction::RestartApp => Self::RestartApp,
        }
    }
}

/// What the window does for an intent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    Change(ReviewChange),
    /// Switches the focused change's file; does nothing when there is none or it has no such switch.
    ToggleFocusedFile(FileToggle),
    /// Opens the overlay, or closes it when open.
    Toggle(Overlay),
    /// Opens Settings, which is closed.
    OpenSettings,
    /// Opens the PR switcher, or leaves it open.
    OpenPrSwitcher,
    AskAboutScreen,
    OpenFinishForm,
    OpenOnGitHub(PullRequestUrl),
    Restart(RestartArgs),
    /// Asks GitHub for a newer release now.
    CheckForUpdates,
    Nothing,
    Refused(Refusal),
}

/// A change of the review an intent makes, applied with `ReviewCore::apply`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReviewChange {
    GoOn { ticking: Option<StepId> },
    GoBack,
    GoToNextUnread,
    ToggleTick(StepId),
    ToggleWrapLines,
    ToggleWhitespace,
}

/// What `R` and `L` switch on the focused change's file, which only the page can say.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileToggle {
    RenderedView,
    PieceLabels,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Overlay {
    Jump,
    PrSwitcher,
    Settings,
    Shortcuts,
}

/// Why an intent does nothing now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// The review is about to be opened again; nothing may reach the state file its build reads.
    Reopening,
    /// Settings is a modal sheet: while it is open, the other overlays wait.
    SettingsOpen,
}

/// What only the window knows when an intent comes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WindowMoment {
    pub reopening: Reopening,
    pub settings: SettingsSheet,
}

/// Whether the PR on show is being opened again, as after Save in Settings.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Reopening {
    Underway,
    #[default]
    NotUnderway,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SettingsSheet {
    Open,
    #[default]
    Closed,
}

/// The arguments of `guided-review restart`, which opens this PR on this screen in a new app.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestartArgs {
    pr: PullRequestUrl,
    at: Option<StepId>,
}

impl RestartArgs {
    pub fn args(&self) -> Vec<String> {
        let mut args = vec!["restart".to_owned(), self.pr.to_string()];
        if let Some(step) = &self.at {
            args.extend(["--at".to_owned(), step.to_string()]);
        }
        args
    }
}

impl Intent {
    fn refusal(self, moment: WindowMoment) -> Option<Refusal> {
        let refused_while_reopening = match self {
            Self::NextScreen
            | Self::PreviousScreen
            | Self::NextUnreadStep
            | Self::ToggleRead
            | Self::ToggleWrapLines
            | Self::ToggleRenderedView
            | Self::TogglePieceLabels
            | Self::ToggleJump
            | Self::ToggleSettings
            | Self::ShowSettings
            | Self::ToggleShortcuts => true,
            Self::MarkReadAndGoOn
            | Self::ToggleWhitespace
            | Self::TogglePrSwitcher
            | Self::ShowPrSwitcher
            | Self::AskAboutScreen
            | Self::FinishReview
            | Self::OpenOnGitHub
            | Self::CheckForUpdates
            | Self::RestartApp => false,
        };
        let waits_for_settings = matches!(
            self,
            Self::ToggleJump | Self::TogglePrSwitcher | Self::ShowPrSwitcher
        );
        if refused_while_reopening && moment.reopening == Reopening::Underway {
            Some(Refusal::Reopening)
        } else if waits_for_settings && moment.settings == SettingsSheet::Open {
            Some(Refusal::SettingsOpen)
        } else {
            None
        }
    }
}

impl ReviewCore {
    pub fn decide(&self, intent: Intent, moment: WindowMoment) -> Effect {
        if let Some(refusal) = intent.refusal(moment) {
            return Effect::Refused(refusal);
        }
        match intent {
            Intent::NextScreen => Effect::Change(ReviewChange::GoOn {
                ticking: self
                    .unread_step_on_screen()
                    .filter(|_| self.has_seen_end_of_screen()),
            }),
            Intent::MarkReadAndGoOn => Effect::Change(ReviewChange::GoOn {
                ticking: self.unread_step_on_screen(),
            }),
            Intent::PreviousScreen => Effect::Change(ReviewChange::GoBack),
            Intent::NextUnreadStep => Effect::Change(ReviewChange::GoToNextUnread),
            Intent::ToggleRead => match self.screen() {
                Screen::Step(id) => Effect::Change(ReviewChange::ToggleTick(id.clone())),
                Screen::Summary | Screen::SafetyNet => Effect::Nothing,
            },
            Intent::ToggleWrapLines => Effect::Change(ReviewChange::ToggleWrapLines),
            Intent::ToggleWhitespace => Effect::Change(ReviewChange::ToggleWhitespace),
            Intent::ToggleRenderedView => Effect::ToggleFocusedFile(FileToggle::RenderedView),
            Intent::TogglePieceLabels => Effect::ToggleFocusedFile(FileToggle::PieceLabels),
            Intent::ToggleJump => Effect::Toggle(Overlay::Jump),
            Intent::TogglePrSwitcher => Effect::Toggle(Overlay::PrSwitcher),
            Intent::ShowPrSwitcher => Effect::OpenPrSwitcher,
            Intent::ToggleSettings => Effect::Toggle(Overlay::Settings),
            Intent::ShowSettings => match moment.settings {
                SettingsSheet::Open => Effect::Nothing,
                SettingsSheet::Closed => Effect::OpenSettings,
            },
            Intent::ToggleShortcuts => Effect::Toggle(Overlay::Shortcuts),
            Intent::AskAboutScreen => Effect::AskAboutScreen,
            Intent::FinishReview => Effect::OpenFinishForm,
            Intent::OpenOnGitHub => Effect::OpenOnGitHub(self.pr().clone()),
            Intent::CheckForUpdates => Effect::CheckForUpdates,
            Intent::RestartApp => Effect::Restart(self.restart_args()),
        }
    }

    /// A tick that cannot be saved is one of the review's problems; the change goes on regardless.
    pub fn apply(&mut self, change: ReviewChange) -> anyhow::Result<()> {
        match change {
            ReviewChange::GoOn { ticking } => {
                let ticked = ticking.map_or(Ok(()), |id| self.tick(&id));
                self.go_on();
                ticked
            }
            ReviewChange::GoBack => {
                self.go_back();
                Ok(())
            }
            ReviewChange::GoToNextUnread => {
                self.go_to_next_unread();
                Ok(())
            }
            ReviewChange::ToggleTick(id) => {
                if self.is_ticked(&id) {
                    self.untick(&id)
                } else {
                    self.tick(&id)
                }
            }
            ReviewChange::ToggleWrapLines => {
                self.display_options_mut().toggle_wrap_lines();
                Ok(())
            }
            ReviewChange::ToggleWhitespace => {
                self.display_options_mut().toggle_ignore_whitespace();
                Ok(())
            }
        }
    }

    /// Where a new app opens this PR: on this step, or where the PR starts.
    pub fn restart_args(&self) -> RestartArgs {
        RestartArgs {
            pr: self.pr().clone(),
            at: match self.screen() {
                Screen::Step(id) => Some(id.clone()),
                Screen::Summary | Screen::SafetyNet => None,
            },
        }
    }

    fn unread_step_on_screen(&self) -> Option<StepId> {
        match self.screen() {
            Screen::Step(id) if !self.is_ticked(id) => Some(id.clone()),
            Screen::Summary | Screen::Step(_) | Screen::SafetyNet => None,
        }
    }
}
