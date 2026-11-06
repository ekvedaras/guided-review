use crate::pull_request::{PullRequestUrl, Sha};

use super::comments::{
    CommentOutcome, CommentPlace, CommentRefusal, CommentRequest, CommentResult, Origin, Outcome,
    QueuedComment, SendMode,
};
use super::finish_review::{
    FinishOutcome, FinishRefusal, FinishRequest, FinishResult, ReviewVerdict,
};
use super::review_state::{ReviewState, ReviewStateFile};
use super::row_selection::{RowSelection, SelectedLines};
use super::step_page::{DiffLine, LineLocation, Side};
use super::suggestion::suggestion_block;
use super::thread_actions::{
    ThreadAction, ThreadActionOutcome, ThreadActionRequest, ThreadActionResult,
};
use super::threads::{CommentId, ReviewThread};
use crate::file_path::FilePath;
use std::collections::BTreeMap;

/// Everything the reviewer writes to GitHub: the drag that opens a comment box, the comments,
/// replies and finished review, and what GitHub said about the pending review and the PR's author.
/// The texts themselves live in the review state; saving it is the review's job.
#[derive(Default)]
pub(super) struct CommentDesk {
    selection: Option<RowSelection>,
    /// "Comment now" is only offered when GitHub is known to have none.
    pending_review: PendingReview,
    /// Approve and Request changes are only offered when the PR is known to be someone else's.
    authorship: PullRequestAuthorship,
    /// The text of each of the viewer's comments being edited. Not saved: an edit not sent is
    /// given up with the app, as the comment on GitHub is still whole.
    comment_edits: BTreeMap<CommentId, String>,
}

#[derive(Default, Clone, Copy, PartialEq, Eq)]
enum PendingReview {
    #[default]
    Unknown,
    Absent,
    Exists,
}

impl PendingReview {
    fn exists(exists: bool) -> Self {
        if exists { Self::Exists } else { Self::Absent }
    }
}

#[derive(Default, Clone, Copy, PartialEq, Eq)]
enum PullRequestAuthorship {
    #[default]
    Unknown,
    OwnedByViewer,
    OwnedBySomeoneElse,
}

/// Whether the tour shows the PR as it is now: a comment on an older tour may land on lines the tour does not show.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TourFreshness {
    Current,
    Behind,
}

impl CommentDesk {
    pub(super) fn pending_review_is(&mut self, exists: bool) {
        self.pending_review = PendingReview::exists(exists);
    }

    pub(super) fn can_comment_now(&self) -> bool {
        self.pending_review == PendingReview::Absent
    }

    pub(super) fn viewer_authorship_is(&mut self, authored: bool) {
        self.authorship = if authored {
            PullRequestAuthorship::OwnedByViewer
        } else {
            PullRequestAuthorship::OwnedBySomeoneElse
        };
    }

    pub(super) fn can_approve_or_request_changes(&self) -> bool {
        self.authorship == PullRequestAuthorship::OwnedBySomeoneElse
    }

    pub(super) fn begin_selection(&mut self, row: LineLocation) {
        self.selection = Some(RowSelection::start_at(row));
    }

    pub(super) fn extend_selection(&mut self, row: LineLocation) {
        if let Some(selection) = &mut self.selection {
            selection.extend_to(row);
        }
    }

    pub(super) fn row_selection(&self) -> Option<&RowSelection> {
        self.selection.as_ref()
    }

    pub(super) fn cancel_selection(&mut self) {
        self.selection = None;
    }

    /// The drag ends: when the rows can take a comment, a draft opens on them.
    pub(super) fn finish_selection(
        &mut self,
        state: &mut ReviewState,
        screen_lines: &[DiffLine],
        freshness: TourFreshness,
        head: &Sha,
    ) -> Result<Option<CommentPlace>, CommentRefusal> {
        let Some(selection) = self.selection.take() else {
            return Ok(None);
        };
        let Some(selected) = selection.resolve(screen_lines) else {
            return Ok(None);
        };
        if !selected.take_comment() {
            return Err(CommentRefusal::NotInsideHunk);
        }
        if freshness == TourFreshness::Behind {
            return Err(CommentRefusal::TourIsOld);
        }
        let place = CommentPlace::of(&selected);
        state.open_draft(place.clone(), Some(head));
        Ok(Some(place))
    }

    pub(super) fn begin_file_comment(
        state: &mut ReviewState,
        path: &FilePath,
        freshness: TourFreshness,
        head: &Sha,
    ) -> Result<CommentPlace, CommentRefusal> {
        if freshness == TourFreshness::Behind {
            return Err(CommentRefusal::TourIsOld);
        }
        let place = CommentPlace::on_file(path);
        state.open_draft(place.clone(), Some(head));
        Ok(place)
    }

    /// Opens a draft on the lines when there is none and puts `text` after what is typed already.
    pub(super) fn add_to_draft(
        state: &mut ReviewState,
        selected: &SelectedLines,
        head: &Sha,
        text: &str,
    ) -> CommentPlace {
        let place = CommentPlace::of(selected);
        state.open_draft(place.clone(), Some(head));
        append_to_draft(state, &place, text);
        place
    }

    /// "Suggest change" shows only when the range starts and ends on lines of the head:
    /// a suggestion replaces lines that stay. Removed rows in between are left out of it.
    pub(super) fn can_suggest_change(place: &CommentPlace) -> bool {
        matches!(place, CommentPlace::Lines { start, end } if start.side == Side::Head && end.side == Side::Head)
    }

    /// Adds a suggestion block of the commented rows to the draft. `None` when the lines cannot be
    /// suggested on or there is no draft. No rows means no block: on GitHub an empty suggestion
    /// would replace the lines with nothing.
    pub(super) fn suggest_change(
        state: &mut ReviewState,
        place: &CommentPlace,
        rows: &[DiffLine],
    ) -> Option<String> {
        if state.draft_at(place).is_none() || !Self::can_suggest_change(place) {
            return None;
        }
        let kept_lines: Vec<&str> = rows
            .iter()
            .filter(|row| row.location.side == Side::Head)
            .map(|row| row.text.as_str())
            .collect();
        if kept_lines.is_empty() {
            return None;
        }
        append_to_draft(state, place, &suggestion_block(&kept_lines))
    }

    /// `None` when there is no such draft.
    pub(super) fn comment_request(
        state: &ReviewState,
        place: &CommentPlace,
        mode: SendMode,
        tour_head: &Sha,
        pr: &PullRequestUrl,
    ) -> Option<CommentRequest> {
        let draft = state.draft_at(place)?.clone();
        Some(CommentRequest::new(
            Origin::Draft,
            draft,
            mode,
            tour_head.clone(),
            pr.clone(),
        ))
    }

    /// The oldest comment in the queue, to send again.
    pub(super) fn queued_comment_request(
        state: &ReviewState,
        tour_head: &Sha,
        pr: &PullRequestUrl,
    ) -> Option<CommentRequest> {
        let queued = state.comment_queue().first()?;
        Some(CommentRequest::new(
            Origin::Queue,
            queued.draft.clone(),
            queued.mode,
            tour_head.clone(),
            pr.clone(),
        ))
    }

    pub(super) fn apply_comment(
        &mut self,
        state: &mut ReviewState,
        outcome: CommentOutcome,
    ) -> CommentResult {
        let CommentOutcome { request, how, .. } = outcome;
        let place = request.draft.place.clone();
        match how {
            Outcome::Sent => {
                self.pending_review = PendingReview::exists(request.mode == SendMode::AddToReview);
                leave(state, &request.origin, &place);
                CommentResult::Sent
            }
            Outcome::Failed(reason) => {
                keep_for_retry(state, &request, &reason);
                CommentResult::Queued(reason)
            }
            Outcome::Rejected(reason) => {
                self.pending_review = PendingReview::Exists;
                keep_as_draft(state, &request);
                CommentResult::Rejected(reason)
            }
            Outcome::Refused(refusal) => {
                if refusal == CommentRefusal::PendingReviewExists {
                    self.pending_review = PendingReview::Exists;
                    if let Some(queued) = state.queued_at(&place) {
                        queued.mode = SendMode::AddToReview;
                    }
                }
                CommentResult::Refused(refusal)
            }
        }
    }

    pub(super) fn thread_action_request(
        &self,
        pr: &PullRequestUrl,
        action: ThreadAction,
        reply_body: Option<String>,
        freshness: TourFreshness,
    ) -> ThreadActionRequest {
        let draft_body = match &action {
            ThreadAction::EditComment { comment_id } => self.comment_edits.get(comment_id).cloned(),
            _ => reply_body,
        };
        ThreadActionRequest {
            pr: pr.clone(),
            action,
            draft_body,
            freshness,
        }
    }

    pub(super) fn comment_edits(&self) -> &BTreeMap<CommentId, String> {
        &self.comment_edits
    }

    /// Opens the edit on the comment's current text, when the viewer wrote it. Another
    /// person's comment is left alone, as is an edit already open.
    pub(super) fn open_edit(&mut self, threads: &[ReviewThread], comment_id: &CommentId) {
        let own_comment = threads
            .iter()
            .flat_map(|thread| &thread.comments)
            .find(|comment| comment.id == *comment_id && comment.by_viewer);
        if let Some(comment) = own_comment {
            self.comment_edits
                .entry(comment_id.clone())
                .or_insert_with(|| comment.body.clone());
        }
    }

    pub(super) fn type_in_edit(&mut self, comment_id: &CommentId, text: &str) {
        if let Some(edit) = self.comment_edits.get_mut(comment_id) {
            text.clone_into(edit);
        }
    }

    pub(super) fn discard_edit(&mut self, comment_id: &CommentId) {
        self.comment_edits.remove(comment_id);
    }

    /// Says whether a reply went out, so its draft was dropped.
    pub(super) fn apply_thread_action(
        &mut self,
        state: &mut ReviewState,
        outcome: &ThreadActionOutcome,
    ) -> bool {
        match (&outcome.result, &outcome.action) {
            (
                ThreadActionResult::Done,
                ThreadAction::Reply {
                    thread_id, mode, ..
                },
            ) => {
                self.pending_review = PendingReview::exists(*mode == SendMode::AddToReview);
                state.drop_reply(thread_id);
                true
            }
            (ThreadActionResult::Done, ThreadAction::EditComment { comment_id }) => {
                self.comment_edits.remove(comment_id);
                false
            }
            (
                ThreadActionResult::Refused(CommentRefusal::PendingReviewExists),
                ThreadAction::Reply { .. },
            ) => {
                self.pending_review = PendingReview::Exists;
                false
            }
            _ => false,
        }
    }

    pub(super) fn finish_review_request(
        &self,
        pr: &PullRequestUrl,
        verdict: ReviewVerdict,
        body: &str,
    ) -> Result<FinishRequest, FinishRefusal> {
        let judges = verdict != ReviewVerdict::Comment;
        if judges && !self.can_approve_or_request_changes() {
            return Err(FinishRefusal::OwnPullRequest);
        }
        let has_text = !body.trim().is_empty();
        let needs_text = match verdict {
            ReviewVerdict::Approve => false,
            ReviewVerdict::RequestChanges => true,
            ReviewVerdict::Comment => self.pending_review != PendingReview::Exists,
        };
        if needs_text && !has_text {
            return Err(FinishRefusal::BodyNeeded(verdict));
        }
        Ok(FinishRequest {
            pr: pr.clone(),
            verdict,
            body: body.trim().to_owned(),
        })
    }

    pub(super) fn apply_finish(
        &mut self,
        state: &mut ReviewState,
        outcome: FinishOutcome,
    ) -> FinishResult {
        if outcome.result == FinishResult::Sent {
            self.pending_review = PendingReview::Absent;
            state.write_finish_body("");
        }
        outcome.result
    }
}

/// Puts `text` after what the draft holds already. `None` when there is no draft at `place`.
impl CommentOutcome {
    /// For a send that ended after its review was dropped, as when the window switched to another PR:
    /// the comment reached GitHub, or failed, and its PR's state file must say so all the same.
    pub fn land_in(self, file: &ReviewStateFile) -> anyhow::Result<()> {
        file.update(|state| {
            CommentDesk::default().apply_comment(state, self);
        })
    }
}

fn append_to_draft(state: &mut ReviewState, place: &CommentPlace, text: &str) -> Option<String> {
    let written = state.draft_at(place)?.body.trim_end().to_owned();
    let body = if written.is_empty() {
        text.to_owned()
    } else {
        format!("{written}\n\n{text}")
    };
    state.write_draft(place, &body);
    Some(body)
}

fn leave(state: &mut ReviewState, origin: &Origin, place: &CommentPlace) {
    match origin {
        Origin::Draft => state.drop_draft(place),
        Origin::Queue => state.drop_queued(place).map(|queued| queued.draft),
    };
}

fn keep_for_retry(state: &mut ReviewState, request: &CommentRequest, reason: &str) {
    let place = &request.draft.place;
    match request.origin {
        Origin::Draft => {
            state.drop_draft(place);
            state.queue_comment(QueuedComment {
                draft: request.draft.clone(),
                mode: request.mode,
                failed_because: reason.to_owned(),
            });
        }
        Origin::Queue => {
            if let Some(queued) = state.queued_at(place) {
                reason.clone_into(&mut queued.failed_because);
            }
        }
    }
}

fn keep_as_draft(state: &mut ReviewState, request: &CommentRequest) {
    let place = &request.draft.place;
    if request.origin == Origin::Queue {
        state.drop_queued(place);
        state.open_draft(place.clone(), request.draft.written_at.as_ref());
        state.write_draft(place, &request.draft.body);
    }
}
