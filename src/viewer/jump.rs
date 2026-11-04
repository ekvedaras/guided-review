use std::borrow::Cow;
use std::collections::HashSet;

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
