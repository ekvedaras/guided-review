use crate::viewer::review_thread::CommentId;
use crate::viewer::review_thread::ReviewThread;
use crate::viewer::review_thread::ThreadComment;
use crate::viewer::review_thread::ThreadId;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use crate::coverage;
use crate::file_path::FilePath;
use crate::pieces::ChangedFile;
use crate::tour::{GuidedTour, Tour};

use super::diff_display::{DiffDisplay, FilesAtHead};
use super::hand_offs::{HandOffButton, HandOffs};
use super::hidden_text::HiddenText;
use super::line_location::{LineLocation, Side};
use super::outdated_lines::FollowedLines;
use super::piece_diff::PieceDiff;
use super::review::StepId;
use super::screen::Screen;
use super::used_findings::{FindingsOfThreads, UsedFinding};
use super::written_on::WrittenOn;

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
    /// The comment of this thread the reviewer is being asked to confirm the deletion of.
    pub delete_asked: Option<CommentId>,
    /// Where the comment sent with Ask agent is, when this thread has one.
    pub hand_off_line: Option<Arc<str>>,
    /// The agent is at work on the thread's latest hand-off, so its line shows it busy.
    pub agent_is_working: bool,
    /// What the reviewer can do about the thread's latest hand-off.
    pub hand_off_button: Option<HandOffButton>,
    /// The comments by others the reviewer can hand to the tour agent now.
    pub can_hand_to_agent: BTreeSet<CommentId>,
    /// What each comment that hides text from its reader hides.
    pub hidden_text: BTreeMap<CommentId, HiddenText>,
    /// An outdated thread's lines, followed to the head, and what they were.
    pub followed: Option<FollowedLines>,
    /// The findings of the AI review its comment was made from.
    pub from_findings: Vec<UsedFinding>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Home {
    Step(StepId),
    SafetyNet,
    File(FilePath),
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

impl<'a> Placed<'a> {
    pub fn len(&self) -> usize {
        self.threads.len()
    }

    pub fn is_empty(&self) -> bool {
        self.threads.is_empty()
    }

    pub fn threads(&self) -> &'a [ReviewThread] {
        self.threads
    }
}

impl ThreadPlacement {
    pub fn of(
        tour: &Tour,
        files: &[ChangedFile],
        threads: &[ReviewThread],
        followed: &BTreeMap<ThreadId, FollowedLines>,
        written_on: &WrittenOn,
        display: &DiffDisplay,
    ) -> Self {
        match tour {
            Tour::Guided(guided) => {
                Self::of_steps(guided, files, threads, (followed, written_on), display)
            }
            Tour::Basic(_) => Self::of_files(files, threads, followed),
        }
    }

    /// A basic tour shows each file whole on a screen of its own, so a thread goes to its file's screen.
    fn of_files(
        files: &[ChangedFile],
        threads: &[ReviewThread],
        followed: &BTreeMap<ThreadId, FollowedLines>,
    ) -> Self {
        let mut homes: BTreeMap<Home, Vec<ReviewThread>> = BTreeMap::new();
        for thread in threads
            .iter()
            .filter(|thread| files.iter().any(|file| file.path == thread.path))
        {
            homes
                .entry(Home::File(thread.path.clone()))
                .or_default()
                .push(thread.clone());
        }
        Self {
            homes,
            followed: followed.clone(),
        }
    }

    /// A thread goes to the step its writer wrote it on while that step still shows its line (or its
    /// file), else to the first step that does, else to the safety net.
    fn of_steps(
        tour: &GuidedTour,
        files: &[ChangedFile],
        threads: &[ReviewThread],
        (followed, written_on): (&BTreeMap<ThreadId, FollowedLines>, &WrittenOn),
        display: &DiffDisplay,
    ) -> Self {
        let steps = step_pieces(tour, files, display);
        let mut homes: BTreeMap<Home, Vec<ReviewThread>> = BTreeMap::new();
        for thread in threads {
            let anchor = thread.anchor(followed.get(&thread.id));
            let shows = |(_, parts): &&(StepId, Vec<PieceDiff>)| match &anchor {
                Some(anchor) => parts.iter().any(|piece| hangs_under_a_row(piece, anchor)),
                None => parts.iter().any(|piece| thread.path == *piece.id.path()),
            };
            let written_on = written_on
                .step_of(&thread.id)
                .and_then(|step| steps.iter().find(|(id, _)| id == step))
                .filter(shows);
            let home = written_on
                .or_else(|| steps.iter().find(shows))
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

    pub fn in_file(&self, path: &FilePath) -> Placed<'_> {
        self.of_home(&Home::File(path.clone()))
    }

    pub fn in_safety_net(&self) -> Placed<'_> {
        self.of_home(&Home::SafetyNet)
    }

    /// The summary and the overview hold no thread.
    pub fn on_screen(&self, screen: &Screen) -> Placed<'_> {
        match screen {
            Screen::Step(id) => self.in_step(id),
            Screen::File(path) => self.in_file(path),
            Screen::SafetyNet => self.in_safety_net(),
            Screen::Summary | Screen::Overview => Placed {
                threads: &[],
                followed: &self.followed,
            },
        }
    }

    fn of_home(&self, home: &Home) -> Placed<'_> {
        Placed {
            threads: self.homes.get(home).map_or(&[], Vec::as_slice),
            followed: &self.followed,
        }
    }
}

/// The code each step shows, in the tour's order.
pub(super) fn step_pieces(
    tour: &GuidedTour,
    files: &[ChangedFile],
    display: &DiffDisplay,
) -> Vec<(StepId, Vec<PieceDiff>)> {
    tour.steps()
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
        .collect()
}

pub(super) fn hangs_under_a_row(piece: &PieceDiff, location: &LineLocation) -> bool {
    piece.lines().any(|row| row.holds(location))
}

/// What the reviewer has begun in the threads and not finished.
pub(super) struct ThreadWork<'a> {
    pub reply_drafts: &'a BTreeMap<ThreadId, String>,
    pub comment_edits: &'a BTreeMap<CommentId, String>,
    pub delete_asked: Option<&'a CommentId>,
}

/// Turns placed threads into what a screen shows. `shown` is the code the screen has.
pub(super) fn views(
    placed: Placed<'_>,
    shown: &[PieceDiff],
    flipped: &BTreeSet<ThreadId>,
    files_at_head: &FilesAtHead,
    work: &ThreadWork<'_>,
    agent: &AgentOfThreads<'_>,
    findings: FindingsOfThreads<'_>,
) -> Vec<ThreadView> {
    let hand_offs = agent.hand_offs;
    let ThreadWork {
        reply_drafts,
        comment_edits,
        delete_asked,
    } = work;
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
            delete_asked: delete_asked
                .filter(|asked| thread.comments.iter().any(|comment| &comment.id == *asked))
                .cloned(),
            hand_off_line: hand_offs.line_for(&thread.id),
            agent_is_working: hand_offs.is_working_on(&thread.id),
            hand_off_button: hand_offs.button_in(&thread.id),
            can_hand_to_agent: thread
                .comments
                .iter()
                .filter(|comment| agent.can_take(comment))
                .map(|comment| comment.id.clone())
                .collect(),
            hidden_text: thread
                .comments
                .iter()
                .filter_map(|comment| {
                    HiddenText::hiding(&comment.body).map(|hidden| (comment.id.clone(), hidden))
                })
                .collect(),
            thread: without_working_on_it(thread, hand_offs),
            from_findings: findings.of_thread(&thread.id),
        })
        .collect()
}

/// What the tour agent has of the threads, and whether it takes comments by others.
pub(super) struct AgentOfThreads<'a> {
    pub hand_offs: &'a HandOffs,
    /// A PR's threads hold comments by others; a local review's hold only the reviewer's and the agent's.
    pub takes_others_comments: bool,
}

impl AgentOfThreads<'_> {
    fn can_take(&self, comment: &ThreadComment) -> bool {
        self.takes_others_comments && !comment.by_viewer && !self.hand_offs.has_at_work(&comment.id)
    }
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

pub(super) fn uncovered(tour: &GuidedTour, files: &[ChangedFile]) -> bool {
    !coverage::safety_net(tour, files).is_empty()
}
