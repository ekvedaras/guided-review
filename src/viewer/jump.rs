use crate::pieces::ChangedFile;
use crate::tour::Tour;

use super::review::{Screen, StepId};
use super::step_page::pieces_of;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JumpKind {
    Step,
    File,
    Symbol,
}

/// One row of the cmd+k list: a place the reviewer can jump to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JumpTarget {
    pub kind: JumpKind,
    /// What the query is matched against and what the row shows first.
    pub label: String,
    /// The step title for files and symbols, empty for steps; "Safety net" for files only it shows.
    pub detail: String,
    pub screen: Screen,
}

/// Steps, then the files and symbols each step shows, in reading order, then the files only the safety net shows.
pub(super) fn all_targets(
    tour: &Tour,
    files: &[ChangedFile],
    safety_net_files: &[&str],
) -> Vec<JumpTarget> {
    let mut targets = Vec::new();
    let mut shown_files: Vec<&str> = Vec::new();
    for step in tour.steps() {
        let id = Screen::Step(StepId::from(step.id.as_str()));
        targets.push(JumpTarget {
            kind: JumpKind::Step,
            label: step.title.clone(),
            detail: String::new(),
            screen: id.clone(),
        });
        for piece in pieces_of(step, tour, files) {
            let path = piece.id.path.as_str();
            if !shown_files.contains(&path) {
                shown_files.push(path);
                targets.push(JumpTarget {
                    kind: JumpKind::File,
                    label: path.to_string(),
                    detail: step.title.clone(),
                    screen: id.clone(),
                });
            }
            if piece.id.symbol.as_deref().is_some_and(is_named_symbol) {
                targets.push(JumpTarget {
                    kind: JumpKind::Symbol,
                    label: piece.id.to_string(),
                    detail: step.title.clone(),
                    screen: id.clone(),
                });
            }
        }
    }
    for path in safety_net_files {
        if !shown_files.contains(path) {
            shown_files.push(path);
            targets.push(JumpTarget {
                kind: JumpKind::File,
                label: (*path).to_string(),
                detail: "Safety net".to_string(),
                screen: Screen::SafetyNet,
            });
        }
    }
    targets
}

/// `hunk-N` pieces only exist because a file could not be cut into symbols; the file row stands for them.
fn is_named_symbol(symbol: &str) -> bool {
    !symbol.starts_with("hunk-")
}

/// Best matches first; targets the query does not spell are dropped. An empty query keeps reading order.
pub(super) fn rank(targets: Vec<JumpTarget>, query: &str) -> Vec<JumpTarget> {
    let query: Vec<char> = query.to_lowercase().chars().collect();
    if query.is_empty() {
        return targets;
    }
    let mut scored: Vec<(usize, usize, JumpTarget)> = targets
        .into_iter()
        .filter_map(|target| {
            let looseness = looseness(&target.label.to_lowercase(), &query)?;
            Some((looseness, target.label.chars().count(), target))
        })
        .collect();
    scored.sort_by_key(|(looseness, length, _)| (*looseness, *length));
    scored.into_iter().map(|(_, _, target)| target).collect()
}

/// Characters between the first and last matched letter that the query does not spell:
/// 0 is an exact run. `None` when the letters do not appear in order.
fn looseness(text: &str, query: &[char]) -> Option<usize> {
    let letters: Vec<char> = text.chars().collect();
    (0..letters.len())
        .filter(|&start| letters[start] == query[0])
        .filter_map(|start| {
            let mut next = start;
            for wanted in query {
                next += letters[next..].iter().position(|c| c == wanted)? + 1;
            }
            Some(next - start - query.len())
        })
        .min()
}
