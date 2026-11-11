use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::Arc;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::coverage;
use crate::file_path::FilePath;
use crate::pieces::ChangedFile;
use crate::tour::Tour;

use super::diff_display::{DiffDisplay, FilesAtHead};
use super::hand_offs::{HandOffId, HandOffs};
use super::outdated_lines::FollowedLines;
use super::reactions::{Reaction, ReactionCount};
use super::review::StepId;
use super::step_page::{LineLocation, PieceDiff, Side};
use crate::line_number::LineNumber;

/// GitHub's node id of a review thread.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ThreadId(Arc<str>);

/// GitHub's node id of one comment in a review thread, which reactions are added to.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CommentId(Arc<str>);

macro_rules! github_node_id {
    ($id:ident) => {
        impl $id {
            pub fn new(id: &str) -> Self {
                Self(id.into())
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $id {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl Serialize for $id {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_str(&self.0)
            }
        }

        impl<'de> Deserialize<'de> for $id {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                Ok(Self::new(&String::deserialize(deserializer)?))
            }
        }
    };
}

github_node_id!(ThreadId);
github_node_id!(CommentId);

/// A review thread as GitHub holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewThread {
    pub id: ThreadId,
    pub is_resolved: bool,
    pub is_outdated: bool,
    pub path: FilePath,
    /// The last line the thread covers at the PR's head. Unknown for outdated threads and file comments.
    pub line: Option<LineNumber>,
    pub start_line: Option<LineNumber>,
    pub side: Side,
    /// A comment on the whole file, not on a line.
    pub is_file_comment: bool,
    pub original_line: Option<LineNumber>,
    pub original_start_line: Option<LineNumber>,
    pub original_commit: Option<String>,
    pub comments: Vec<ThreadComment>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreadComment {
    pub id: CommentId,
    pub author: String,
    pub is_bot: bool,
    /// The viewer wrote it, so the viewer may edit it.
    pub by_viewer: bool,
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
    /// Where the thread hangs: its line, or for an outdated thread where its lines were followed to.
    pub fn anchor(&self, followed: Option<&FollowedLines>) -> Option<LineLocation> {
        if self.is_file_comment {
            return None;
        }
        if self.is_outdated {
            return followed.map(|followed| LineLocation {
                path: self.path.clone(),
                side: Side::Head,
                line: followed.end,
            });
        }
        Some(LineLocation {
            path: self.path.clone(),
            side: self.side,
            line: self.line?,
        })
    }

    pub(super) fn is_folded_by_default(&self) -> bool {
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
    pub commented_lines: Arc<[Arc<str>]>,
    /// The reply being written in this thread; `Some` means the reply box is open.
    pub reply_draft: Option<String>,
    /// The text of each comment of this thread that is being edited.
    pub edit_drafts: BTreeMap<CommentId, String>,
    /// Where the comment sent with Ask agent is, when this thread has one.
    pub hand_off_line: Option<Arc<str>>,
    /// The thread's latest hand-off failed and can be tried again.
    pub failed_hand_off: Option<HandOffId>,
    /// An outdated thread's lines, followed to the head, and what they were.
    pub followed: Option<FollowedLines>,
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
    followed: BTreeMap<ThreadId, FollowedLines>,
}

/// The threads of one screen, with where their outdated lines went.
#[derive(Clone, Copy)]
pub(super) struct Placed<'a> {
    threads: &'a [ReviewThread],
    followed: &'a BTreeMap<ThreadId, FollowedLines>,
}

impl Placed<'_> {
    pub fn len(&self) -> usize {
        self.threads.len()
    }

    pub fn is_empty(&self) -> bool {
        self.threads.is_empty()
    }
}

impl ThreadPlacement {
    pub fn of(
        tour: &Tour,
        files: &[ChangedFile],
        threads: &[ReviewThread],
        followed: &BTreeMap<ThreadId, FollowedLines>,
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
                (step.id.clone(), parts)
            })
            .collect();
        let mut homes: BTreeMap<Home, Vec<ReviewThread>> = BTreeMap::new();
        for thread in threads {
            let home = steps
                .iter()
                .find(|(_, parts)| match thread.anchor(followed.get(&thread.id)) {
                    Some(anchor) => parts.iter().any(|piece| hangs_under_a_row(piece, &anchor)),
                    None => parts.iter().any(|piece| thread.path == *piece.id.path()),
                })
                .map_or(Home::SafetyNet, |(id, _)| Home::Step(id.clone()));
            homes.entry(home).or_default().push(thread.clone());
        }
        Self {
            homes,
            followed: followed.clone(),
        }
    }

    pub fn in_step(&self, id: &StepId) -> Placed<'_> {
        self.of_home(&Home::Step(id.clone()))
    }

    pub fn in_safety_net(&self) -> Placed<'_> {
        self.of_home(&Home::SafetyNet)
    }

    fn of_home(&self, home: &Home) -> Placed<'_> {
        Placed {
            threads: self.homes.get(home).map_or(&[], Vec::as_slice),
            followed: &self.followed,
        }
    }
}

fn hangs_under_a_row(piece: &PieceDiff, location: &LineLocation) -> bool {
    piece.lines().any(|row| row.holds(location))
}

/// Turns placed threads into what a screen shows. `shown` is the code the screen has.
pub(super) fn views(
    placed: Placed<'_>,
    shown: &[PieceDiff],
    flipped: &BTreeSet<ThreadId>,
    files_at_head: &FilesAtHead,
    reply_drafts: &BTreeMap<ThreadId, String>,
    comment_edits: &BTreeMap<CommentId, String>,
    hand_offs: &HandOffs,
) -> Vec<ThreadView> {
    placed
        .threads
        .iter()
        .map(|thread| ThreadView {
            is_folded: thread.is_folded_by_default() != flipped.contains(&thread.id),
            anchor: thread
                .anchor(placed.followed.get(&thread.id))
                .filter(|anchor| shown.iter().any(|piece| hangs_under_a_row(piece, anchor))),
            followed: placed.followed.get(&thread.id).cloned(),
            commented_lines: commented_lines(thread, files_at_head),
            reply_draft: reply_drafts.get(&thread.id).cloned(),
            edit_drafts: thread
                .comments
                .iter()
                .filter_map(|comment| {
                    let text = comment_edits.get(&comment.id)?;
                    Some((comment.id.clone(), text.clone()))
                })
                .collect(),
            hand_off_line: hand_offs.line_for(&thread.id),
            failed_hand_off: hand_offs.failed_in(&thread.id),
            thread: without_working_on_it(thread, hand_offs),
        })
        .collect()
}

/// The line under the thread already says the agent is working on it, so its reply saying so is left out.
fn without_working_on_it(thread: &ReviewThread, hand_offs: &HandOffs) -> ReviewThread {
    let mut shown = thread.clone();
    if let Some(reply) = hand_offs.working_on_it_in(&thread.id) {
        shown.comments.retain(|comment| &comment.id != reply);
    }
    shown
}

fn commented_lines(thread: &ReviewThread, files_at_head: &FilesAtHead) -> Arc<[Arc<str>]> {
    let has_suggestion = thread
        .comments
        .iter()
        .any(|comment| comment.body.contains("```suggestion"));
    match (has_suggestion, thread.is_outdated, thread.side, thread.line) {
        (true, false, Side::Head, Some(last)) => files_at_head
            .lines(
                thread.path.as_str(),
                thread.start_line.unwrap_or(last).into(),
                last.into(),
            )
            .into_iter()
            .map(Arc::from)
            .collect(),
        _ => Arc::new([]),
    }
}

pub(super) fn uncovered(tour: &Tour, files: &[ChangedFile]) -> bool {
    !coverage::safety_net(tour, files).is_empty()
}
