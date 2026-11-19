//! What the reader asks of the review's window, by key, button or palette, and what that does.
//!
//! The window reports what it alone knows (an overlay open, a reopen under way, the end of the
//! screen on show) and carries out the `Effect`; the rules that pick the effect are here, once.

use super::file_tree::FolderPath;
use super::guided_tour::GuidedTourRefusal;
use super::jump::PaletteAction;
use super::review::{ReadSubject, ReviewCore, Screen};
use crate::issue::IssueLink;
use crate::pull_request::PullRequestUrl;
use crate::review_subject::ReviewSubject;
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
    /// `O`, on the open screen's boxes.
    ToggleFold,
    ToggleJump,
    TogglePrSwitcher,
    /// The PR switcher asked for from outside the window, by a launch from the Desktop entry.
    ShowPrSwitcher,
    ToggleSettings,
    /// Settings asked for from outside the window, by `guided-review settings`.
    ShowSettings,
    ToggleShortcuts,
    AskAboutScreen,
    /// Write guided tour, from the overview, the sidebar or the palette.
    WriteGuidedTour,
    SwitchToGuidedTour,
    FinishReview,
    /// A re-review with nothing new to send is done, from the sidebar, the finish card or the palette.
    MarkDone,
    /// A local review's finish, from the sidebar, the finish card or the palette.
    SendTasks,
    /// A local review's ask to its agent to open the PR of its branch.
    OpenPr,
    OpenOnGitHub,
    /// GitHub's checks page of the PR.
    OpenChecksOnGitHub,
    /// A row of the title bar's check list, by its place in the list; the row past the last is the
    /// "and more" one. Opens the check's run, else the checks page.
    OpenCheck(usize),
    /// `I`: opens the review's issue, or lists them when there are several.
    OpenIssue,
    /// A row of the issue list, by its place in it.
    OpenIssueAt(usize),
    /// `D`: asks, and on the same press again soon after, converts the PR to a draft or marks it ready.
    ToggleDraft,
    /// `Shift+M`: asks, and on the same press again soon after, turns auto-merge on or off, or
    /// merges now when nothing blocks the PR.
    ToggleAutoMerge,
    CheckForUpdates,
    RestartApp,
}

/// A palette action does what its button or key does.
impl From<PaletteAction> for Intent {
    fn from(action: PaletteAction) -> Self {
        match action {
            PaletteAction::NextUnreadStep => Self::NextUnreadStep,
            PaletteAction::MarkStepRead
            | PaletteAction::MarkStepUnread
            | PaletteAction::MarkFileRead
            | PaletteAction::MarkFileUnread => Self::ToggleRead,
            PaletteAction::WrapLines | PaletteAction::StopWrappingLines => Self::ToggleWrapLines,
            PaletteAction::HideWhitespace | PaletteAction::ShowWhitespace => Self::ToggleWhitespace,
            PaletteAction::ShowPieceLabels | PaletteAction::HidePieceLabels => {
                Self::TogglePieceLabels
            }
            PaletteAction::ShowRendered | PaletteAction::ShowCode => Self::ToggleRenderedView,
            PaletteAction::OpenAllPieces | PaletteAction::FoldAllPieces => Self::ToggleFold,
            PaletteAction::AskAboutStep => Self::AskAboutScreen,
            PaletteAction::WriteGuidedTour => Self::WriteGuidedTour,
            PaletteAction::SwitchToGuidedTour => Self::SwitchToGuidedTour,
            PaletteAction::FinishReview => Self::FinishReview,
            PaletteAction::MarkDone => Self::MarkDone,
            PaletteAction::SendTasks => Self::SendTasks,
            PaletteAction::OpenPr => Self::OpenPr,
            PaletteAction::OpenOnGitHub => Self::OpenOnGitHub,
            PaletteAction::OpenIssue => Self::OpenIssue,
            PaletteAction::OpenChecksOnGitHub => Self::OpenChecksOnGitHub,
            PaletteAction::ConvertToDraft | PaletteAction::MarkReadyForReview => Self::ToggleDraft,
            PaletteAction::EnableAutoMerge
            | PaletteAction::DisableAutoMerge
            | PaletteAction::MergeNow => Self::ToggleAutoMerge,
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
    /// Opens or folds every box of the screen; does nothing where the screen has no fold toggle.
    ToggleScreenFold,
    /// Opens the overlay, or closes it when open.
    Toggle(Overlay),
    /// Opens Settings, which is closed.
    OpenSettings,
    /// Opens the PR switcher, or leaves it open.
    OpenPrSwitcher,
    AskAboutScreen,
    /// Starts the guided tour's writing: the review took it and the app hands the task to the agent.
    WriteGuidedTour,
    /// The reviewer asked to switch to a ready guided tour.
    SwitchToGuidedTour,
    OpenFinishForm,
    /// Hands a local review's queued tasks to its agent.
    SendTasks,
    /// Asks the local review's agent to open the PR of its branch.
    OpenPr,
    OpenOnGitHub(PullRequestUrl),
    OpenChecksOnGitHub(PullRequestUrl),
    OpenIssue(IssueLink),
    /// The review is about several issues: the window lists them to pick one.
    ListIssues,
    /// A check's run, on the page GitHub gave.
    OpenCheck(String),
    /// The window asks the review to change the PR between draft and ready, and says what it answers.
    ChangeReadiness,
    /// The window asks the review to turn auto-merge on or off or merge now, and says what it answers.
    ChangeMerging,
    Restart(RestartArgs),
    /// Asks GitHub for a newer release now.
    CheckForUpdates,
    Nothing,
    Refused(Refusal),
}

/// A change of the review an intent makes, applied with `ReviewCore::apply`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReviewChange {
    GoOn {
        ticking: Option<ReadSubject>,
    },
    GoBack,
    GoToNextUnread,
    ToggleTick(ReadSubject),
    /// A folder of a basic tour's sidebar tree folds or unfolds.
    ToggleFolder(FolderPath),
    ToggleWrapLines,
    ToggleWhitespace,
    MarkDone,
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
    /// A guided-tour action that does not apply now.
    GuidedTour(GuidedTourRefusal),
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

/// The arguments of `guided-review restart`, which opens this review subject on this screen in a new app.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestartArgs {
    subject: ReviewSubject,
    at: Option<StepId>,
}

impl RestartArgs {
    /// `app_build` names the app restarting, which the restart may quit though its binary, reinstalled
    /// or updated, is another build: only a restart the reviewer asked for ends an app of another build.
    pub fn args(&self, app_build: &str) -> Vec<String> {
        let mut args = vec!["restart".to_owned(), self.subject.to_string()];
        if let Some(step) = &self.at {
            args.extend(["--at".to_owned(), step.to_string()]);
        }
        args.extend(["--replacing".to_owned(), app_build.to_owned()]);
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
            | Self::MarkDone
            | Self::ToggleWrapLines
            | Self::ToggleRenderedView
            | Self::TogglePieceLabels
            | Self::ToggleFold
            | Self::ToggleDraft
            | Self::ToggleAutoMerge
            | Self::ToggleJump
            | Self::ToggleSettings
            | Self::ShowSettings
            | Self::ToggleShortcuts => true,
            Self::MarkReadAndGoOn
            | Self::ToggleWhitespace
            | Self::TogglePrSwitcher
            | Self::ShowPrSwitcher
            | Self::AskAboutScreen
            | Self::WriteGuidedTour
            | Self::SwitchToGuidedTour
            | Self::FinishReview
            | Self::SendTasks
            | Self::OpenPr
            | Self::OpenOnGitHub
            | Self::OpenChecksOnGitHub
            | Self::OpenCheck(_)
            | Self::OpenIssue
            | Self::OpenIssueAt(_)
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
                    .unread_subject_on_screen()
                    .filter(|_| self.has_seen_end_of_screen()),
            }),
            Intent::MarkReadAndGoOn => Effect::Change(ReviewChange::GoOn {
                ticking: self.unread_subject_on_screen(),
            }),
            Intent::PreviousScreen => Effect::Change(ReviewChange::GoBack),
            Intent::NextUnreadStep => Effect::Change(ReviewChange::GoToNextUnread),
            Intent::ToggleRead => match self.screen() {
                Screen::Step(id) => {
                    Effect::Change(ReviewChange::ToggleTick(ReadSubject::Step(id.clone())))
                }
                Screen::File(path) => {
                    Effect::Change(ReviewChange::ToggleTick(ReadSubject::File(path.clone())))
                }
                Screen::Summary | Screen::Overview | Screen::SafetyNet => Effect::Nothing,
            },
            Intent::ToggleWrapLines => Effect::Change(ReviewChange::ToggleWrapLines),
            Intent::ToggleWhitespace => Effect::Change(ReviewChange::ToggleWhitespace),
            Intent::ToggleRenderedView => Effect::ToggleFocusedFile(FileToggle::RenderedView),
            Intent::TogglePieceLabels => Effect::ToggleFocusedFile(FileToggle::PieceLabels),
            Intent::ToggleFold => Effect::ToggleScreenFold,
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
            Intent::WriteGuidedTour => match self.write_guided_tour_refusal() {
                None => Effect::WriteGuidedTour,
                Some(refusal) => Effect::Refused(Refusal::GuidedTour(refusal)),
            },
            Intent::SwitchToGuidedTour => match self.switch_to_guided_tour() {
                Ok(()) => Effect::SwitchToGuidedTour,
                Err(refusal) => Effect::Refused(Refusal::GuidedTour(refusal)),
            },
            Intent::MarkDone if self.can_mark_done() => Effect::Change(ReviewChange::MarkDone),
            Intent::FinishReview | Intent::SendTasks => match self.finishes_with_send_tasks() {
                Some(0) => Effect::Nothing,
                Some(_) => Effect::SendTasks,
                None => Effect::OpenFinishForm,
            },
            Intent::OpenPr if self.can_open_pr() => Effect::OpenPr,
            Intent::OpenPr | Intent::MarkDone => Effect::Nothing,
            Intent::OpenOnGitHub => self
                .subject()
                .pull_request()
                .map_or(Effect::Nothing, |pr| Effect::OpenOnGitHub(pr.clone())),
            Intent::OpenChecksOnGitHub => self
                .subject()
                .pull_request()
                .map_or(Effect::Nothing, |pr| Effect::OpenChecksOnGitHub(pr.clone())),
            Intent::OpenCheck(place) => {
                let page = self
                    .pr_status()
                    .and_then(|status| status.open())
                    .and_then(|status| status.check_list().shown.into_iter().nth(place))
                    .and_then(|check| check.page);
                match (page, self.subject().pull_request()) {
                    (Some(page), _) => Effect::OpenCheck(page.to_string()),
                    (None, Some(pr)) => Effect::OpenChecksOnGitHub(pr.clone()),
                    (None, None) => Effect::Nothing,
                }
            }
            Intent::OpenIssue => match &*self.related_issues() {
                [] => Effect::Nothing,
                [only] => Effect::OpenIssue(only.link.clone()),
                [_, _, ..] => Effect::ListIssues,
            },
            Intent::OpenIssueAt(place) => self
                .related_issues()
                .get(place)
                .map_or(Effect::Nothing, |issue| {
                    Effect::OpenIssue(issue.link.clone())
                }),
            Intent::ToggleDraft => Effect::ChangeReadiness,
            Intent::ToggleAutoMerge => Effect::ChangeMerging,
            Intent::CheckForUpdates => Effect::CheckForUpdates,
            Intent::RestartApp => Effect::Restart(self.restart_args()),
        }
    }

    /// A tick that cannot be saved is one of the review's problems; the change goes on regardless.
    pub fn apply(&mut self, change: ReviewChange) -> anyhow::Result<()> {
        match change {
            ReviewChange::GoOn { ticking } => {
                let ticked = ticking.map_or(Ok(()), |subject| self.tick_subject(&subject));
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
            ReviewChange::ToggleTick(subject) => {
                if self.is_read(&subject) {
                    self.untick_subject(&subject)
                } else {
                    self.tick_subject(&subject)
                }
            }
            ReviewChange::ToggleFolder(folder) => {
                self.toggle_folder(&folder);
                Ok(())
            }
            ReviewChange::ToggleWrapLines => {
                self.display_options_mut().toggle_wrap_lines();
                Ok(())
            }
            ReviewChange::ToggleWhitespace => {
                self.display_options_mut().toggle_ignore_whitespace();
                Ok(())
            }
            ReviewChange::MarkDone => self.mark_done(),
        }
    }

    /// Where a new app opens this PR: on this step, or where the PR starts.
    pub fn restart_args(&self) -> RestartArgs {
        RestartArgs {
            subject: self.subject().clone(),
            at: match self.screen() {
                Screen::Step(id) => Some(id.clone()),
                Screen::Summary | Screen::Overview | Screen::File(_) | Screen::SafetyNet => None,
            },
        }
    }

    fn tick_subject(&mut self, subject: &ReadSubject) -> anyhow::Result<()> {
        match subject {
            ReadSubject::Step(id) => self.tick(id),
            ReadSubject::File(path) => self.tick_file(path),
        }
    }

    fn untick_subject(&mut self, subject: &ReadSubject) -> anyhow::Result<()> {
        match subject {
            ReadSubject::Step(id) => self.untick(id),
            ReadSubject::File(path) => self.untick_file(path),
        }
    }

    fn unread_subject_on_screen(&self) -> Option<ReadSubject> {
        match self.screen() {
            Screen::Step(id) if !self.is_ticked(id) => Some(ReadSubject::Step(id.clone())),
            Screen::File(path) if !self.is_file_ticked(path) => {
                Some(ReadSubject::File(path.clone()))
            }
            Screen::Summary
            | Screen::Overview
            | Screen::File(_)
            | Screen::Step(_)
            | Screen::SafetyNet => None,
        }
    }
}
