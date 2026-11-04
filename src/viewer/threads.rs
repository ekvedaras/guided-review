use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use crate::coverage;
use crate::pieces::ChangedFile;
use crate::tour::Tour;

use super::diff_display::{DiffDisplay, FilesAtHead};
use super::reactions::{Reaction, ReactionCount};
use super::review::StepId;
use super::step_page::{LineLocation, PieceDiff, Side};

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
    /// GitHub's node id, which reactions are added to.
    pub id: String,
    pub author: String,
    pub is_bot: bool,
    pub body: String,
    /// The comment on the PR conversation page.
    pub url: String,
    /// Only the reactions someone made; `reaction_summary` fills in the rest.
    pub reactions: Vec<ReactionCount>,
}

impl ThreadComment {
    /// All eight reactions in GitHub's order, with zero for the ones nobody used.
    pub fn reaction_summary(&self) -> Vec<ReactionCount> {
        Reaction::ALL
            .into_iter()
            .map(|reaction| {
                self.reactions
                    .iter()
                    .find(|count| count.reaction == reaction)
                    .copied()
                    .unwrap_or(ReactionCount {
                        reaction,
                        count: 0,
                        viewer_has_reacted: false,
                    })
            })
            .collect()
    }
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
    /// The lines of the head the thread is on, for showing suggestions against them.
    /// Empty unless a comment holds a suggestion.
    pub commented_lines: Arc<[String]>,
    /// The reply being written in this thread; `Some` means the reply box is open.
    pub reply_draft: Option<String>,
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
    pub fn of(
        tour: &Tour,
        files: &[ChangedFile],
        threads: &[ReviewThread],
        display: &DiffDisplay,
    ) -> Self {
        let steps: Vec<(StepId, Vec<PieceDiff>)> = tour
            .steps()
            .into_iter()
            .map(|step| {
                let parts = step
                    .pieces
                    .iter()
                    .flat_map(|pointer| coverage::shown_parts(pointer, tour, files))
                    .filter_map(|part| PieceDiff::of(&part, files, display))
                    .collect();
                (StepId::from(step.id.as_str()), parts)
            })
            .collect();
        let mut homes: BTreeMap<Home, Vec<ReviewThread>> = BTreeMap::new();
        for thread in threads {
            let home = steps
                .iter()
                .find(|(_, parts)| match thread.anchor() {
                    Some(anchor) => parts.iter().any(|piece| hangs_under_a_row(piece, &anchor)),
                    None => parts.iter().any(|piece| piece.id.path == thread.path),
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

fn hangs_under_a_row(piece: &PieceDiff, location: &LineLocation) -> bool {
    piece.lines().any(|row| row.holds(location))
}

/// Turns placed threads into what a screen shows. `shown` is the code the screen has.
pub(super) fn views(
    threads: &[ReviewThread],
    shown: &[PieceDiff],
    flipped: &BTreeSet<String>,
    files_at_head: &FilesAtHead,
    reply_drafts: &BTreeMap<String, String>,
) -> Vec<ThreadView> {
    threads
        .iter()
        .map(|thread| ThreadView {
            is_folded: thread.is_folded_by_default() != flipped.contains(&thread.id),
            anchor: thread
                .anchor()
                .filter(|anchor| shown.iter().any(|piece| hangs_under_a_row(piece, anchor))),
            commented_lines: commented_lines(thread, files_at_head),
            reply_draft: reply_drafts.get(&thread.id).cloned(),
            thread: thread.clone(),
        })
        .collect()
}

fn commented_lines(thread: &ReviewThread, files_at_head: &FilesAtHead) -> Arc<[String]> {
    let has_suggestion = thread
        .comments
        .iter()
        .any(|comment| comment.body.contains("```suggestion"));
    match (has_suggestion, thread.is_outdated, thread.side, thread.line) {
        (true, false, Side::Head, Some(last)) => files_at_head
            .lines(&thread.path, thread.start_line.unwrap_or(last), last)
            .into(),
        _ => Arc::new([]),
    }
}

pub(super) fn uncovered(tour: &Tour, files: &[ChangedFile]) -> bool {
    !coverage::safety_net(tour, files).is_empty()
}
