use crate::pull_request::{PullRequestUrl, Sha};

use super::comments::{
    CommentOutcome, CommentPlace, CommentRefusal, CommentRequest, CommentResult, Origin, Outcome,
    QueuedComment, SendMode,
};
use super::finish_review::{
    FinishOutcome, FinishRefusal, FinishRequest, FinishResult, ReviewVerdict,
};
use super::review_state::ReviewState;
use super::row_selection::{RowSelection, SelectedLines};
use super::step_page::{DiffLine, LineLocation, Side};
use super::suggestion::suggestion_block;
use super::thread_actions::{
    ThreadAction, ThreadActionOutcome, ThreadActionRequest, ThreadActionResult,
};

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
        tour_is_old: bool,
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
        if tour_is_old {
            return Err(CommentRefusal::TourIsOld);
        }
        let place = CommentPlace::of(&selected);
        state.open_draft(place.clone(), head);
        Ok(Some(place))
    }

    pub(super) fn begin_file_comment(
        &self,
        state: &mut ReviewState,
        path: &str,
        tour_is_old: bool,
        head: &Sha,
    ) -> Result<CommentPlace, CommentRefusal> {
        if tour_is_old {
            return Err(CommentRefusal::TourIsOld);
        }
        let place = CommentPlace::on_file(path);
        state.open_draft(place.clone(), head);
        Ok(place)
    }

    /// Opens a draft on the lines when there is none and puts `text` after what is typed already.
    pub(super) fn add_to_draft(
        &self,
        state: &mut ReviewState,
        selected: &SelectedLines,
        head: &Sha,
        text: &str,
    ) -> CommentPlace {
        let place = CommentPlace::of(selected);
        state.open_draft(place.clone(), head);
        append_to_draft(state, &place, text);
        place
    }

    /// "Suggest change" shows only when the range starts and ends on lines of the head:
    /// a suggestion replaces lines that stay. Removed rows in between are left out of it.
    pub(super) fn can_suggest_change(place: &CommentPlace) -> bool {
        !place.is_file_comment() && place.start.side == Side::Head && place.end.side == Side::Head
    }

    /// Adds a suggestion block of the commented rows to the draft. `None` when the lines cannot be
    /// suggested on or there is no draft.
    pub(super) fn suggest_change(
        &self,
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
        append_to_draft(state, place, &suggestion_block(&kept_lines))
    }

    /// `None` when there is no such draft.
    pub(super) fn comment_request(
        &self,
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
        &self,
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
        let CommentOutcome { request, how } = outcome;
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
        tour_is_old: bool,
    ) -> ThreadActionRequest {
        ThreadActionRequest {
            pr: pr.clone(),
            action,
            tour_is_old,
        }
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
            body: body.trim().to_string(),
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
fn append_to_draft(state: &mut ReviewState, place: &CommentPlace, text: &str) -> Option<String> {
    let written = state.draft_at(place)?.body.trim_end().to_string();
    let body = if written.is_empty() {
        text.to_string()
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
                failed_because: reason.to_string(),
            });
        }
        Origin::Queue => {
            if let Some(queued) = state.queued_at(place) {
                queued.failed_because = reason.to_string();
            }
        }
    }
}

fn keep_as_draft(state: &mut ReviewState, request: &CommentRequest) {
    let place = &request.draft.place;
    if request.origin == Origin::Queue {
        state.drop_queued(place);
        state.open_draft(place.clone(), &request.draft.written_at);
        state.write_draft(place, &request.draft.body);
    }
}
