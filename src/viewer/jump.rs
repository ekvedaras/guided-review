use std::borrow::Cow;
use std::collections::HashSet;

use super::shortcuts::Command;
use crate::file_path::FilePath;
use crate::pieces::{ChangedFile, PieceId};
use crate::tour::Tour;

use super::review::Screen;
use super::step_page::pieces_of;

/// Where on its screen a jump lands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JumpPlace {
    StepTop,
    File(FilePath),
    Piece(PieceId),
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
    /// What the query is matched against: the step's title, the file's path or the piece's id.
    pub fn label(&self) -> Cow<'_, str> {
        match &self.place {
            JumpPlace::StepTop => Cow::Borrowed(&self.screen_title),
            JumpPlace::File(path) => Cow::Borrowed(path.as_str()),
            JumpPlace::Piece(id) => Cow::Owned(id.to_string()),
        }
    }

    /// Where a file or symbol is shown; a step needs none.
    pub fn detail(&self) -> Option<&str> {
        match self.place {
            JumpPlace::StepTop => None,
            JumpPlace::File(_) | JumpPlace::Piece(_) => Some(&self.screen_title),
        }
    }
}

/// Steps, then the files and symbols each step shows, in reading order, then the files only the safety net shows.
pub(super) fn all_targets(
    tour: &Tour,
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
    MarkStepRead,
    MarkStepUnread,
    WrapLines,
    StopWrappingLines,
    HideWhitespace,
    ShowWhitespace,
    /// On the focused change's file.
    ShowPieceLabels,
    HidePieceLabels,
    ShowRendered,
    ShowCode,
    AskAboutStep,
    FinishReview,
    OpenOnGitHub,
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
            Self::MarkStepRead => "Mark step as read",
            Self::MarkStepUnread => "Mark step as unread",
            Self::WrapLines => "Wrap lines",
            Self::StopWrappingLines => "Stop wrapping lines",
            Self::HideWhitespace => "Hide whitespace",
            Self::ShowWhitespace => "Show whitespace",
            Self::ShowPieceLabels => "Show piece labels in this file",
            Self::HidePieceLabels => "Hide piece labels in this file",
            Self::ShowRendered => "Show this file rendered",
            Self::ShowCode => "Show this file as code",
            Self::AskAboutStep => "Ask about this step",
            Self::FinishReview => "Finish review",
            Self::OpenOnGitHub => "Open on GitHub",
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
            Self::MarkStepRead | Self::MarkStepUnread => Some(Command::ToggleRead),
            Self::WrapLines | Self::StopWrappingLines => Some(Command::WrapLines),
            Self::ShowPieceLabels | Self::HidePieceLabels => Some(Command::TogglePieceLabels),
            Self::ShowRendered | Self::ShowCode => Some(Command::ToggleRenderedView),
            Self::Settings => Some(Command::Settings),
            Self::Shortcuts => Some(Command::Shortcuts),
            Self::HideWhitespace
            | Self::ShowWhitespace
            | Self::AskAboutStep
            | Self::FinishReview
            | Self::OpenOnGitHub
            | Self::SwitchPr
            | Self::CheckForUpdates
            | Self::RestartApp => None,
        }
    }
}

/// The palette's headings, in the order they show when nothing is typed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaletteGroupKind {
    Steps,
    Files,
    Symbols,
    Actions,
}

impl PaletteGroupKind {
    pub fn title(self) -> &'static str {
        match self {
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
                JumpPlace::StepTop => PaletteGroupKind::Steps,
                JumpPlace::File(_) => PaletteGroupKind::Files,
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
