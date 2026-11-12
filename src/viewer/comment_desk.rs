use crate::pull_request::{PullRequestUrl, Sha};

use super::ask::Ask;
use super::comments::{
    CommentDraft, CommentOutcome, CommentPlace, CommentProblem, CommentRefusal, CommentRequest,
    CommentResult, Origin, Outcome, QueuedComment, SendMode,
};
use super::finish_review::{
    FinishOutcome, FinishRefusal, FinishRequest, FinishResult, ReviewVerdict,
};
use super::hand_offs::{Audience, SentComment};
use super::review::StepId;
use super::review_state::{ReviewState, ReviewStateFile};
use super::row_selection::{RowSelection, SelectedLines};
use super::saved_state::SavedState;
use super::step_page::{DiffLine, LineLocation, Side};
use super::suggestion::suggestion_block;
use super::thread_actions::{
    ThreadAction, ThreadActionOutcome, ThreadActionRequest, ThreadActionResult,
};
use super::threads::{CommentId, ReviewThread, ThreadId};
use crate::file_path::FilePath;
use std::collections::{BTreeMap, HashMap, HashSet};

/// Everything the reviewer writes to GitHub: the drag that opens a comment box, the comments,
/// replies and finished review, and what GitHub said about the pending review and the PR's author.
/// The texts themselves live in the review state and change only through its gate, `SavedState`.
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
    /// The comment box that takes the keys.
    composing: Option<CommentPlace>,
    /// Comment boxes whose text is on its way to GitHub.
    sending: HashSet<CommentPlace>,
    /// Why the last send from a comment box did not go, until it is sent again or given up.
    send_problems: HashMap<CommentPlace, CommentProblem>,
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

/// One box the reviewer writes text in, named by what its text becomes on GitHub.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TextBox {
    /// A new comment on lines or on a file.
    Draft(CommentPlace),
    Reply(ThreadId),
    /// New text for one of the reviewer's own comments.
    Edit(CommentId),
    /// The body of the review being finished.
    FinishBody,
}

/// What the reviewer did to open a comment box.
#[derive(Debug, Clone, Copy)]
pub enum CommentGesture<'a> {
    /// The drag over rows ended; a click is a drag over one row.
    DragEnded,
    /// The comment button under a picture: the comment is on the whole file.
    OnFile(&'a FilePath),
    /// Use as comment on an Ask: its latest answer goes into the box on its lines.
    AskAnswer(&'a Ask),
}

/// A comment box as it is drawn: its draft, what it is doing and what stops it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommentBox<'a> {
    pub draft: &'a CommentDraft,
    pub state: BoxState,
    /// Why the last send did not go, or else the tour being behind the PR.
    pub problem: Option<CommentProblem>,
}

/// What a comment box is doing: only one of these at a time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BoxState {
    /// Shown, but keys go elsewhere.
    Idle,
    /// Takes the keys.
    Composing,
    /// On its way to GitHub.
    Sending,
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

    pub(super) fn can_finish_review(&self) -> bool {
        self.can_approve_or_request_changes() || self.pending_review == PendingReview::Exists
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

    /// The drag ends: when the rows can take a comment, a draft opens on them and takes the keys.
    pub(super) fn finish_selection(
        &mut self,
        state: &mut SavedState,
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
        if let Some(refusal) = selected.comment_refusal() {
            return Err(refusal);
        }
        if freshness == TourFreshness::Behind {
            return Err(CommentRefusal::TourIsOld);
        }
        let place = CommentPlace::of(&selected);
        self.open_draft(state, &place, head, None)?;
        Ok(Some(place))
    }

    pub(super) fn begin_file_comment(
        &mut self,
        state: &mut SavedState,
        path: &FilePath,
        freshness: TourFreshness,
        head: &Sha,
    ) -> Result<CommentPlace, CommentRefusal> {
        if freshness == TourFreshness::Behind {
            return Err(CommentRefusal::TourIsOld);
        }
        let place = CommentPlace::on_file(path);
        self.open_draft(state, &place, head, None)?;
        Ok(place)
    }

    /// Opens a draft on the lines when there is none and puts `text` after what is typed already.
    pub(super) fn add_to_draft(
        &mut self,
        state: &mut SavedState,
        selected: &SelectedLines,
        head: &Sha,
        text: &str,
    ) -> Result<CommentPlace, CommentRefusal> {
        let place = CommentPlace::of(selected);
        self.open_draft(state, &place, head, Some(text))?;
        Ok(place)
    }

    /// The box is open in memory even when its save fails; the reviewer learns its text is not
    /// kept, and the box does not take the keys.
    fn open_draft(
        &mut self,
        state: &mut SavedState,
        place: &CommentPlace,
        head: &Sha,
        appended: Option<&str>,
    ) -> Result<(), CommentRefusal> {
        state
            .record(|state| {
                state.open_draft(place.clone(), Some(head));
                if let Some(text) = appended {
                    append_to_draft(state, place, text);
                }
            })
            .map_err(|error| CommentRefusal::NoAccess(format!("{error:#}")))?;
        self.composing = Some(place.clone());
        Ok(())
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
        state: &mut SavedState,
        place: &CommentPlace,
        rows: &[DiffLine],
    ) -> anyhow::Result<Option<String>> {
        if state.draft_at(place).is_none() || !Self::can_suggest_change(place) {
            return Ok(None);
        }
        let kept_lines: Vec<&str> = rows
            .iter()
            .filter(|row| row.location.side == Side::Head)
            .map(|row| row.text.as_str())
            .collect();
        if kept_lines.is_empty() {
            return Ok(None);
        }
        state.record(|state| append_to_draft(state, place, &suggestion_block(&kept_lines)))
    }

    pub(super) fn composing(&self) -> Option<&CommentPlace> {
        self.composing.as_ref()
    }

    pub(super) fn comment_boxes<'a>(
        &'a self,
        state: &'a ReviewState,
        freshness: TourFreshness,
    ) -> impl Iterator<Item = CommentBox<'a>> {
        state.comment_drafts().iter().map(move |draft| {
            let place = &draft.place;
            let box_state = if self.sending.contains(place) {
                BoxState::Sending
            } else if self.composing.as_ref() == Some(place) {
                BoxState::Composing
            } else {
                BoxState::Idle
            };
            let problem = self.send_problems.get(place).cloned().or_else(|| {
                (freshness == TourFreshness::Behind)
                    .then_some(CommentProblem::Refused(CommentRefusal::TourIsOld))
            });
            CommentBox {
                draft,
                state: box_state,
                problem,
            }
        })
    }

    /// The box shows as sending, with no problem, until the outcome is applied. `None` when there
    /// is no such draft.
    pub(super) fn start_sending(
        &mut self,
        state: &ReviewState,
        place: &CommentPlace,
        mode: SendMode,
        audience: Audience,
        (tour_head, pr): (&Sha, &PullRequestUrl),
    ) -> Option<CommentRequest> {
        let draft = state.draft_at(place)?.clone();
        self.sending.insert(place.clone());
        self.send_problems.remove(place);
        Some(CommentRequest::new(
            Origin::Draft,
            draft,
            mode,
            audience,
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
            queued.audience,
            tour_head.clone(),
            pr.clone(),
        ))
    }

    /// An error means the outcome is in the state but its save failed. A comment box shows how
    /// its send ended; a queued comment has no box.
    pub(super) fn apply_comment(
        &mut self,
        state: &mut SavedState,
        outcome: CommentOutcome,
        step: Option<StepId>,
    ) -> anyhow::Result<CommentResult> {
        let from_box =
            (outcome.request.origin == Origin::Draft).then(|| outcome.request.draft.place.clone());
        let result = state.record(|state| self.settle(state, outcome, step));
        if let Some(place) = from_box {
            self.show_landing(&place, &result);
        }
        result
    }

    /// A sent or queued comment closes the box; any other keeps it, with why.
    fn show_landing(&mut self, place: &CommentPlace, result: &anyhow::Result<CommentResult>) {
        self.sending.remove(place);
        let problem = match result {
            Ok(CommentResult::Sent | CommentResult::Queued(_)) => {
                self.composing = None;
                return;
            }
            Ok(CommentResult::Refused(refusal)) => CommentProblem::Refused(refusal.clone()),
            Ok(CommentResult::Rejected(reason)) => CommentProblem::Rejected(reason.as_str().into()),
            Err(error) => CommentProblem::Failed(format!("{error:#}").into()),
        };
        self.send_problems.insert(place.clone(), problem);
    }

    fn settle(
        &mut self,
        state: &mut ReviewState,
        outcome: CommentOutcome,
        step: Option<StepId>,
    ) -> CommentResult {
        let CommentOutcome {
            request,
            how,
            threads,
        } = outcome;
        let place = request.draft.place.clone();
        match how {
            Outcome::Sent => {
                self.pending_review = PendingReview::exists(request.mode == SendMode::AddToReview);
                leave(state, &request.origin, &place);
                if request.audience == Audience::Agent {
                    state.hand_offs_mut().record_new_thread(
                        place.path(),
                        &SentComment {
                            place: place.describe(),
                            step,
                            mode: request.mode,
                            body: &request.draft.body,
                        },
                        threads.as_ref(),
                    );
                }
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

    /// `None` while the box is not open; the finish body counts as open once it has text.
    pub(super) fn text_of<'a>(
        &'a self,
        state: &'a ReviewState,
        text_box: &TextBox,
    ) -> Option<&'a str> {
        match text_box {
            TextBox::Draft(place) => state.draft_at(place).map(|draft| draft.body.as_str()),
            TextBox::Reply(thread_id) => state.reply_draft(thread_id),
            TextBox::Edit(comment_id) => self.comment_edits.get(comment_id).map(String::as_str),
            TextBox::FinishBody => Some(state.finish_body()).filter(|body| !body.is_empty()),
        }
    }

    /// A draft opens only from a gesture on lines, which checks them first, so here it is left as
    /// it is; the finish body is always there to type in.
    pub(super) fn open(
        &mut self,
        state: &mut SavedState,
        threads: &[ReviewThread],
        text_box: &TextBox,
    ) -> anyhow::Result<()> {
        match text_box {
            TextBox::Draft(place) => {
                if state.draft_at(place).is_some() {
                    self.composing = Some(place.clone());
                }
                Ok(())
            }
            TextBox::FinishBody => Ok(()),
            TextBox::Reply(thread_id) => state.record(|state| state.open_reply(thread_id)),
            TextBox::Edit(comment_id) => {
                self.open_edit(threads, comment_id);
                Ok(())
            }
        }
    }

    /// For every key: changes the text in memory only. The typing save takes saved boxes to disk,
    /// off the UI thread; an edit is not saved.
    pub(super) fn type_in(
        &mut self,
        state: &mut SavedState,
        text_box: &TextBox,
        text: &str,
    ) -> anyhow::Result<()> {
        match text_box {
            TextBox::Draft(place) => state.type_in(|state| state.write_draft(place, text)),
            TextBox::Reply(thread_id) => state.type_in(|state| state.write_reply(thread_id, text)),
            TextBox::Edit(comment_id) => {
                state.refuse_while_repinning()?;
                if let Some(edit) = self.comment_edits.get_mut(comment_id) {
                    text.clone_into(edit);
                }
                Ok(())
            }
            TextBox::FinishBody => state.type_in(|state| state.write_finish_body(text)),
        }
    }

    /// The reviewer leaves the box (Escape): a box with nothing written goes, a written one stays.
    pub(super) fn leave(
        &mut self,
        state: &mut SavedState,
        text_box: &TextBox,
    ) -> anyhow::Result<()> {
        if let TextBox::Draft(place) = text_box {
            self.stop_composing(place);
        }
        let is_blank = self
            .text_of(state, text_box)
            .is_some_and(|text| text.trim().is_empty());
        if is_blank {
            self.discard(state, text_box)?;
        }
        Ok(())
    }

    /// A draft goes together with the queued comment at its place, which is shown in its stead.
    pub(super) fn discard(
        &mut self,
        state: &mut SavedState,
        text_box: &TextBox,
    ) -> anyhow::Result<()> {
        match text_box {
            TextBox::Draft(place) => {
                self.stop_composing(place);
                self.send_problems.remove(place);
                state.record(|state| {
                    state.drop_draft(place);
                    state.drop_queued(place);
                })
            }
            TextBox::Reply(thread_id) => state.record(|state| state.drop_reply(thread_id)),
            TextBox::Edit(comment_id) => {
                self.comment_edits.remove(comment_id);
                Ok(())
            }
            TextBox::FinishBody => state.record(|state| state.write_finish_body("")),
        }
    }

    fn stop_composing(&mut self, place: &CommentPlace) {
        if self.composing.as_ref() == Some(place) {
            self.composing = None;
        }
    }

    /// Opens the edit on the comment's current text, when the viewer wrote it. Another
    /// person's comment is left alone, as is an edit already open.
    fn open_edit(&mut self, threads: &[ReviewThread], comment_id: &CommentId) {
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

    pub(super) fn apply_thread_action(
        &mut self,
        state: &mut SavedState,
        outcome: &ThreadActionOutcome,
        step: Option<StepId>,
    ) {
        match (&outcome.result, &outcome.action) {
            (
                ThreadActionResult::Done,
                ThreadAction::Reply {
                    thread_id,
                    mode,
                    audience,
                },
            ) => {
                self.pending_review = PendingReview::exists(*mode == SendMode::AddToReview);
                let sent = state.record(|state| {
                    if *audience == Audience::Agent {
                        hand_off_reply(state, thread_id, *mode, step, outcome);
                    }
                    state.drop_reply(thread_id);
                });
                // The reply is on GitHub; a draft that fails to clear is saved again with the next typing.
                if sent.is_err() {
                    state.save_with_next_typing();
                }
            }
            (ThreadActionResult::Done, ThreadAction::EditComment { comment_id }) => {
                self.comment_edits.remove(comment_id);
            }
            (
                ThreadActionResult::Refused(CommentRefusal::PendingReviewExists),
                ThreadAction::Reply { .. },
            ) => {
                self.pending_review = PendingReview::Exists;
            }
            _ => {}
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
        state: &mut SavedState,
        outcome: FinishOutcome,
    ) -> FinishResult {
        if outcome.result == FinishResult::Sent {
            self.pending_review = PendingReview::Absent;
            // The review is on GitHub; a body that fails to clear is saved again with the next typing.
            if state.record(|state| state.write_finish_body("")).is_err() {
                state.save_with_next_typing();
            }
        }
        outcome.result
    }
}

impl CommentOutcome {
    /// For a send that ended after its review was dropped, as when the window switched to another PR:
    /// the comment reached GitHub, or failed, and its PR's state file must say so all the same.
    pub fn land_in(self, file: &ReviewStateFile) -> anyhow::Result<()> {
        file.update(|state| {
            CommentDesk::default().settle(state, self, None);
        })
    }
}

/// Puts `text` after what the draft holds already. `None` when there is no draft at `place`.
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
                audience: request.audience,
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

fn hand_off_reply(
    state: &mut ReviewState,
    thread_id: &ThreadId,
    mode: SendMode,
    step: Option<StepId>,
    outcome: &ThreadActionOutcome,
) {
    state.hand_offs_mut().record_reply(
        thread_id,
        SentComment {
            place: String::new(),
            step,
            mode,
            body: outcome.reply_body.as_deref().unwrap_or_default(),
        },
        outcome.threads.as_ref(),
    );
}
