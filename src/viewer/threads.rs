use std::collections::{BTreeMap, BTreeSet};

use crate::coverage::{self, PiecePart};
use crate::pieces::ChangedFile;
use crate::tour::Tour;

use super::review::StepId;
use super::step_page::{LineLocation, Side};

/// A review thread as GitHub holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewThread {
    pub id: String,
    pub is_resolved: bool,
    pub is_outdated: bool,
    pub path: String,
    /// The last line the thread covers at the PR's head. Unknown for outdated threads and file comments.
    pub line: Option<usize>,
    pub start_line: Option<usize>,
    pub side: Side,
    /// A comment on the whole file, not on a line.
    pub is_file_comment: bool,
    pub original_line: Option<usize>,
    pub original_start_line: Option<usize>,
    pub original_commit: Option<String>,
    pub comments: Vec<ThreadComment>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreadComment {
    pub author: String,
    pub is_bot: bool,
    pub body: String,
    /// The comment on the PR conversation page.
    pub url: String,
}

impl ReviewThread {
    /// Where the thread hangs, for a thread that still points at a line.
    pub fn anchor(&self) -> Option<LineLocation> {
        if self.is_outdated || self.is_file_comment {
            return None;
        }
        Some(LineLocation {
            path: self.path.clone(),
            side: self.side,
            line: self.line?,
        })
    }

    fn is_folded_by_default(&self) -> bool {
        self.is_resolved
            || self.is_outdated
            || self.comments.first().is_some_and(|comment| comment.is_bot)
    }
}

/// A thread as one screen shows it.
#[derive(Debug, Clone)]
pub struct ThreadView {
    pub thread: ReviewThread,
    pub is_folded: bool,
    /// The shown line it hangs under; `None` for threads shown after the code (outdated, file comments,
    /// and threads on lines that only the safety net could not show).
    pub anchor: Option<LineLocation>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Home {
    Step(StepId),
    SafetyNet,
}

/// Which screen holds which thread. Computed once per pull; the reviewer's folds live elsewhere.
#[derive(Debug, Default)]
pub(super) struct ThreadPlacement {
    homes: BTreeMap<Home, Vec<ReviewThread>>,
}

impl ThreadPlacement {
    pub fn of(tour: &Tour, files: &[ChangedFile], threads: &[ReviewThread]) -> Self {
        let steps: Vec<(StepId, Vec<PiecePart>)> = tour
            .steps()
            .into_iter()
            .map(|step| {
                let parts = step
                    .pieces
                    .iter()
                    .flat_map(|pointer| pointer.shown_parts(tour, files))
                    .collect();
                (StepId::from(step.id.as_str()), parts)
            })
            .collect();
        let mut homes: BTreeMap<Home, Vec<ReviewThread>> = BTreeMap::new();
        for thread in threads {
            let home = steps
                .iter()
                .find(|(_, parts)| match thread.anchor() {
                    Some(anchor) => parts.iter().any(|part| holds(part, &anchor)),
                    None => parts.iter().any(|part| part.piece.id.path == thread.path),
                })
                .map_or(Home::SafetyNet, |(id, _)| Home::Step(id.clone()));
            homes.entry(home).or_default().push(thread.clone());
        }
        Self { homes }
    }

    pub fn in_step(&self, id: &StepId) -> &[ReviewThread] {
        self.of_home(&Home::Step(id.clone()))
    }

    pub fn in_safety_net(&self) -> &[ReviewThread] {
        self.of_home(&Home::SafetyNet)
    }

    fn of_home(&self, home: &Home) -> &[ReviewThread] {
        self.homes.get(home).map_or(&[], Vec::as_slice)
    }
}

/// GitHub takes comments on changed lines and on the unchanged lines around them in the diff.
const GITHUB_CONTEXT_LINES: usize = 3;

fn holds(part: &PiecePart, location: &LineLocation) -> bool {
    part.piece.id.path == location.path
        && match location.side {
            Side::Base => part
                .removed_lines
                .iter()
                .any(|line| line.number == location.line),
            Side::Head => part
                .added_lines
                .iter()
                .any(|line| line.number.abs_diff(location.line) <= GITHUB_CONTEXT_LINES),
        }
}

/// Turns placed threads into what a screen shows. `shown` is the code the screen has.
pub(super) fn views(
    threads: &[ReviewThread],
    shown: &[PiecePart],
    flipped: &BTreeSet<String>,
) -> Vec<ThreadView> {
    threads
        .iter()
        .map(|thread| ThreadView {
            is_folded: thread.is_folded_by_default() != flipped.contains(&thread.id),
            anchor: thread
                .anchor()
                .filter(|anchor| shown.iter().any(|part| holds(part, anchor))),
            thread: thread.clone(),
        })
        .collect()
}

pub(super) fn uncovered(tour: &Tour, files: &[ChangedFile]) -> bool {
    !coverage::safety_net(tour, files).is_empty()
}
