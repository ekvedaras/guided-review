use std::borrow::Cow;
use std::collections::HashSet;

use crate::pull_request::HostSite;

use crate::file_path::FilePath;
use crate::keys::Command;
use crate::pieces::{ChangedFile, PieceId};
use crate::tour::GuidedTour;

use super::line_location::LineLocation;
use super::piece_diff::pieces_of;
use super::screen::Screen;

/// Where on its screen a jump lands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JumpPlace {
    StepTop,
    /// A basic tour's overview.
    Overview,
    File(FilePath),
    Piece(PieceId),
    /// A line a Definition card points at, as the page shows it: a row's own location.
    Line(LineLocation),
}

/// One row of the cmd+k list: a place the reviewer can jump to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JumpTarget {
    pub screen: Screen,
    pub place: JumpPlace,
    /// The step's title, or "Safety net" for files only it shows.
    screen_title: String,
}

impl JumpTarget {
    /// A file's box on a screen that shows it, `screen_title` naming that screen.
    pub(super) fn file_on(screen: Screen, path: FilePath, screen_title: String) -> Self {
        Self {
            screen,
            place: JumpPlace::File(path),
            screen_title,
        }
    }

    /// What the query is matched against: the step's title, the file's path or the piece's id.
    pub fn label(&self) -> Cow<'_, str> {
        match &self.place {
            JumpPlace::StepTop | JumpPlace::Overview => Cow::Borrowed(&self.screen_title),
            JumpPlace::File(path) => Cow::Borrowed(path.as_str()),
            JumpPlace::Line(line) => Cow::Borrowed(line.path.as_str()),
            JumpPlace::Piece(id) => Cow::Owned(id.to_string()),
        }
    }

    /// The same screen, landing on `line`.
    pub(super) fn at_line(&self, line: LineLocation) -> Self {
        Self {
            place: JumpPlace::Line(line),
            ..self.clone()
        }
    }

    /// Where a file or symbol is shown; a step needs none, nor does a file that has a screen of its own.
    pub fn detail(&self) -> Option<&str> {
        match (&self.place, &self.screen) {
            (JumpPlace::StepTop | JumpPlace::Overview, _) | (_, Screen::File(_)) => None,
            (JumpPlace::File(_) | JumpPlace::Line(_) | JumpPlace::Piece(_), _) => {
                Some(&self.screen_title)
            }
        }
    }
}

/// The overview, then a file screen for each changed file in `files_in_reading_order`.
pub(super) fn basic_targets(files_in_reading_order: &[FilePath]) -> Vec<JumpTarget> {
    let overview = JumpTarget {
        screen: Screen::Overview,
        place: JumpPlace::Overview,
        screen_title: "Overview".to_owned(),
    };
    let files = files_in_reading_order.iter().map(|path| JumpTarget {
        screen: Screen::File(path.clone()),
        place: JumpPlace::File(path.clone()),
        screen_title: path.to_string(),
    });
    std::iter::once(overview).chain(files).collect()
}

/// Steps, then the files and symbols each step shows, in reading order, then the files only the safety net shows.
pub(super) fn all_targets(
    tour: &GuidedTour,
    files: &[ChangedFile],
    safety_net_files: &[&FilePath],
) -> Vec<JumpTarget> {
    let mut targets = Vec::new();
    let mut shown_files: HashSet<&FilePath> = HashSet::new();
    for step in tour.steps() {
        let id = Screen::Step(step.id.clone());
        targets.push(JumpTarget {
            screen: id.clone(),
            place: JumpPlace::StepTop,
            screen_title: step.title.clone(),
        });
        for piece in pieces_of(step, tour, files) {
            let path = piece.id.path();
            if shown_files.insert(path) {
                targets.push(JumpTarget {
                    screen: id.clone(),
                    place: JumpPlace::File(path.clone()),
                    screen_title: step.title.clone(),
                });
            }
            // A `hunk-N` piece only exists because the file could not be cut into symbols; the file row stands for it.
            if piece.id.symbol_path().is_some() && !piece.id.is_hunk() {
                targets.push(JumpTarget {
                    screen: id.clone(),
                    place: JumpPlace::Piece(piece.id.clone()),
                    screen_title: step.title.clone(),
                });
            }
        }
    }
    for path in safety_net_files {
        if shown_files.insert(path) {
            targets.push(JumpTarget {
                screen: Screen::SafetyNet,
                place: JumpPlace::File((*path).clone()),
                screen_title: "Safety net".to_owned(),
            });
        }
    }
    targets
}

/// Best matches first; targets the query does not spell are dropped. An empty query keeps reading order.
pub(super) fn rank(targets: Vec<JumpTarget>, query: &str) -> Vec<JumpTarget> {
    let query_text = query.to_lowercase();
    let query: Vec<char> = query_text.chars().collect();
    if query.is_empty() {
        return targets;
    }
    let mut scored: Vec<(usize, usize, JumpTarget)> = targets
        .into_iter()
        .filter_map(|target| {
            let (looseness, length) = {
                let label = target.label();
                (
                    looseness(&label, &query, &query_text)?,
                    label.chars().count(),
                )
            };
            Some((looseness, length, target))
        })
        .collect();
    scored.sort_by_key(|(looseness, length, _)| (*looseness, *length));
    scored.into_iter().map(|(_, _, target)| target).collect()
}

/// Characters between the first and last matched letter that the query does not spell:
/// 0 is an exact run. The query spells the text in runs, and every run but one found
/// whole in the text starts a word, so letters scattered over a long path match nothing.
/// `None` when it does not spell the text so.
fn looseness(text: &str, query: &[char], query_text: &str) -> Option<usize> {
    let lowercase = text.to_lowercase();
    let original: Vec<char> = text.chars().collect();
    let letters: Vec<char> = lowercase.chars().collect();
    if original.len() != letters.len() {
        return None;
    }
    if lowercase.contains(query_text) {
        return Some(0);
    }
    let (first_letter, rest_of_query) = query.split_first()?;
    let starts_word = |at: usize| {
        at == 0
            || match (original.get(at - 1), original.get(at)) {
                (Some(before), Some(here)) => {
                    !before.is_alphanumeric() || (before.is_lowercase() && here.is_uppercase())
                }
                _ => false,
            }
    };
    letters
        .iter()
        .enumerate()
        .filter(|&(start, letter)| letter == first_letter && starts_word(start))
        .filter_map(|(start, _)| {
            let mut last = start;
            for wanted in rest_of_query {
                last = if letters.get(last + 1) == Some(wanted) {
                    last + 1
                } else {
                    (last + 1..letters.len())
                        .find(|&at| letters.get(at) == Some(wanted) && starts_word(at))?
                };
            }
            Some(last + 1 - start - query.len())
        })
        .min()
}

/// Something the palette can do rather than a place it can go to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PaletteAction {
    NextUnreadStep,
    /// To the next finding of the AI review, most severe first.
    NextFinding,
    /// Opens the list of the AI review's findings.
    FindingsList,
    /// Opens the list of the review's threads.
    CommentsList,
    /// Opens the changed files as a tree.
    FilesList,
    MarkStepRead,
    MarkStepUnread,
    /// A basic tour's read tick is per file.
    MarkFileRead,
    MarkFileUnread,
    WrapLines,
    StopWrappingLines,
    HideWhitespace,
    ShowWhitespace,
    /// On the focused change's file.
    ShowPieceLabels,
    HidePieceLabels,
    ShowRendered,
    ShowCode,
    /// On the open screen's boxes, where the toolbar has the fold toggle.
    OpenAllPieces,
    FoldAllPieces,
    /// Asks the PR's tour agent to write the guided tour of a basic review.
    WriteGuidedTour,
    SwitchToGuidedTour,
    /// Starts the review agent: the AI review of the subject.
    RunAiReview,
    FinishReview,
    /// A re-review with nothing new to send: done, with no review on the host.
    MarkDone,
    /// A local review's finish: its queued tasks go to the change set's agent.
    SendTasks,
    /// A local review's ask to its agent to open the PR of its branch.
    OpenPr,
    /// The PR's page on its host, named after the host.
    OpenOnHost(HostSite),
    /// The PR's checks page on its host.
    OpenChecksOnHost(HostSite),
    /// Offered only where the review knows its issue; lists them when there are several.
    OpenIssue,
    /// Offered only where the PR is ready and the viewer may change it.
    ConvertToDraft,
    /// Offered only where the PR is a draft and the viewer may change it.
    MarkReadyForReview,
    /// Offered only where the host lets the viewer turn it on and something blocks the merge.
    EnableAutoMerge,
    /// Offered only while auto-merge is on and the host lets the viewer turn it off.
    DisableAutoMerge,
    /// Offered only where nothing blocks the merge and the viewer may merge.
    MergeNow,
    SwitchPr,
    Settings,
    Shortcuts,
    /// Asks GitHub for a newer release now, not when the next timed check comes.
    CheckForUpdates,
    /// Quits and opens this PR on this screen in a new app, as after an install.
    RestartApp,
}

impl PaletteAction {
    pub fn label(self) -> &'static str {
        match self {
            Self::NextUnreadStep => "Next unread step",
            Self::NextFinding => "Next finding",
            Self::FindingsList => "Findings list",
            Self::CommentsList => "Comments list",
            Self::FilesList => "Files list",
            Self::MarkStepRead => "Mark step as read",
            Self::MarkStepUnread => "Mark step as unread",
            Self::MarkFileRead => "Mark file as read",
            Self::MarkFileUnread => "Mark file as unread",
            Self::WrapLines => "Wrap lines",
            Self::StopWrappingLines => "Stop wrapping lines",
            Self::HideWhitespace => "Hide whitespace",
            Self::ShowWhitespace => "Show whitespace",
            Self::ShowPieceLabels => "Show piece labels in this file",
            Self::HidePieceLabels => "Hide piece labels in this file",
            Self::ShowRendered => "Show this file rendered",
            Self::ShowCode => "Show this file as code",
            Self::OpenAllPieces => "Open all pieces",
            Self::FoldAllPieces => "Fold all pieces",
            Self::WriteGuidedTour => "Write guided tour",
            Self::SwitchToGuidedTour => "Switch to guided tour",
            Self::RunAiReview => "Run AI review",
            Self::FinishReview => "Finish review",
            Self::MarkDone => "Mark review done",
            Self::SendTasks => "Send tasks",
            Self::OpenPr => "Open PR",
            Self::OpenOnHost(HostSite::GitHub) => "Open on GitHub",
            Self::OpenChecksOnHost(HostSite::GitHub) => "Open checks on GitHub",
            Self::OpenIssue => "Open related issue",
            Self::ConvertToDraft => "Convert to draft",
            Self::MarkReadyForReview => "Mark as ready for review",
            Self::EnableAutoMerge => "Enable auto-merge",
            Self::DisableAutoMerge => "Disable auto-merge",
            Self::MergeNow => "Merge now",
            Self::SwitchPr => "Switch pull request",
            Self::Settings => "Settings",
            Self::Shortcuts => "Keyboard shortcuts",
            Self::CheckForUpdates => "Check for updates",
            Self::RestartApp => "Restart app here",
        }
    }

    /// The shortcut that does the same, for its key caps.
    pub fn command(self) -> Option<Command> {
        match self {
            Self::NextUnreadStep => Some(Command::NextUnreadStep),
            Self::FindingsList => Some(Command::FindingsList),
            Self::CommentsList => Some(Command::CommentsList),
            Self::FilesList => Some(Command::FilesList),
            Self::MarkStepRead
            | Self::MarkStepUnread
            | Self::MarkFileRead
            | Self::MarkFileUnread => Some(Command::ToggleRead),
            Self::WrapLines | Self::StopWrappingLines => Some(Command::WrapLines),
            Self::ShowPieceLabels | Self::HidePieceLabels => Some(Command::TogglePieceLabels),
            Self::ShowRendered | Self::ShowCode => Some(Command::ToggleRenderedView),
            Self::OpenAllPieces | Self::FoldAllPieces => Some(Command::ToggleFold),
            Self::ConvertToDraft | Self::MarkReadyForReview => Some(Command::ToggleDraft),
            Self::EnableAutoMerge | Self::DisableAutoMerge | Self::MergeNow => {
                Some(Command::ToggleAutoMerge)
            }
            Self::Settings => Some(Command::Settings),
            Self::OpenIssue => Some(Command::OpenIssue),
            Self::OpenPr => Some(Command::OpenPr),
            Self::SwitchPr => Some(Command::SwitchPr),
            Self::Shortcuts => Some(Command::Shortcuts),
            Self::HideWhitespace | Self::ShowWhitespace => Some(Command::HideWhitespace),
            Self::NextFinding
            | Self::WriteGuidedTour
            | Self::SwitchToGuidedTour
            | Self::RunAiReview
            | Self::FinishReview
            | Self::MarkDone
            | Self::SendTasks
            | Self::OpenOnHost(_)
            | Self::OpenChecksOnHost(_)
            | Self::CheckForUpdates
            | Self::RestartApp => None,
        }
    }
}

/// The palette's headings, in the order they show when nothing is typed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaletteGroupKind {
    Overview,
    Steps,
    Files,
    Symbols,
    Actions,
}

impl PaletteGroupKind {
    pub fn title(self) -> &'static str {
        match self {
            Self::Overview => "Overview",
            Self::Steps => "Steps",
            Self::Files => "Files",
            Self::Symbols => "Symbols",
            Self::Actions => "Actions",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaletteEntry {
    Place(JumpTarget),
    Action(PaletteAction),
}

/// The entries under one heading, best match first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaletteGroup {
    pub group: PaletteGroupKind,
    pub entries: Vec<PaletteEntry>,
}

/// Places grouped by kind, the group with the best match first, then the actions that match. A query
/// that starts with `>` asks for actions only.
pub(super) fn palette(
    places: Vec<JumpTarget>,
    actions: &[PaletteAction],
    query: &str,
) -> Vec<PaletteGroup> {
    let (actions_only, query) = match query.trim_start().strip_prefix('>') {
        Some(rest) => (true, rest.trim()),
        None => (false, query),
    };
    let mut groups: Vec<PaletteGroup> = Vec::new();
    if !actions_only {
        for target in rank(places, query) {
            let kind = match target.place {
                JumpPlace::Overview => PaletteGroupKind::Overview,
                JumpPlace::StepTop => PaletteGroupKind::Steps,
                JumpPlace::File(_) | JumpPlace::Line(_) => PaletteGroupKind::Files,
                JumpPlace::Piece(_) => PaletteGroupKind::Symbols,
            };
            let entry = PaletteEntry::Place(target);
            match groups.iter_mut().find(|group| group.group == kind) {
                Some(group) => group.entries.push(entry),
                None => groups.push(PaletteGroup {
                    group: kind,
                    entries: vec![entry],
                }),
            }
        }
    }
    let actions = rank_actions(actions, query);
    if !actions.is_empty() {
        groups.push(PaletteGroup {
            group: PaletteGroupKind::Actions,
            entries: actions.into_iter().map(PaletteEntry::Action).collect(),
        });
    }
    groups
}

/// Like `rank`, on the actions' names.
fn rank_actions(actions: &[PaletteAction], query: &str) -> Vec<PaletteAction> {
    let query_text = query.to_lowercase();
    let query: Vec<char> = query_text.chars().collect();
    if query.is_empty() {
        return actions.to_vec();
    }
    let mut scored: Vec<(usize, usize, PaletteAction)> = actions
        .iter()
        .filter_map(|action| {
            let label = action.label();
            let looseness = looseness(label, &query, &query_text)?;
            Some((looseness, label.chars().count(), *action))
        })
        .collect();
    scored.sort_by_key(|(looseness, length, _)| (*looseness, *length));
    scored.into_iter().map(|(_, _, action)| action).collect()
}
