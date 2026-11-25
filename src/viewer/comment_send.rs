//! Sending one comment: to the host, or into a local review's state file, on a background thread.

use super::comments::{Audience, CommentDraft, CommentRefusal, QueuedComment, SendMode};
use super::hand_offs::{SentComment, SentFrom};
use super::local_thread_store::{LocalThreadStore, ThreadHome};
use super::ports::{ChangeTipsPort, HostError, ReviewThreadsPort, ReviewWritesPort};
use super::review_state::ReviewState;
use super::review_thread::{ReviewThread, ThreadId};
use super::used_findings::thread_made_of;
use crate::findings::Finding;
use crate::pull_request::{PullRequestUrl, Sha};
use crate::viewer::comments::CommentPlace;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Origin {
    Draft,
    Queue,
}

/// Everything sending one comment needs, owned, so it can run on a background thread.
pub struct CommentRequest {
    origin: Origin,
    draft: CommentDraft,
    mode: SendMode,
    audience: Audience,
    tour_head: Sha,
    home: ThreadHome,
}

pub struct CommentOutcome {
    pub(super) request: CommentRequest,
    pub(super) how: Outcome,
    /// The threads as the host has them after the comment went in, read in the same background run,
    /// so the comment box is replaced by its thread in one step; `None` when nothing was sent.
    pub(super) threads: Option<Result<Vec<ReviewThread>, String>>,
}

pub(super) enum Outcome {
    Sent,
    Failed(String),
    Rejected(String),
    Refused(CommentRefusal),
}

impl CommentOutcome {
    /// The thread the comment became, among the threads read after it went in.
    pub(super) fn thread_made(&self, known_before: &[ReviewThread]) -> Option<ThreadId> {
        thread_made_of(&self.request.draft, self.threads.as_ref(), known_before)
    }
}

impl CommentRequest {
    pub(super) fn new(
        origin: Origin,
        draft: CommentDraft,
        mode: SendMode,
        audience: Audience,
        tour_head: Sha,
        home: ThreadHome,
    ) -> Self {
        Self {
            origin,
            draft,
            mode,
            audience,
            tour_head,
            home,
        }
    }

    pub const fn place(&self) -> &CommentPlace {
        &self.draft.place
    }

    pub const fn audience(&self) -> Audience {
        self.audience
    }

    pub(super) fn is_from_a_box(&self) -> bool {
        self.origin == Origin::Draft
    }

    pub(super) fn findings_carried(&self) -> &[Finding] {
        &self.draft.from_findings
    }

    /// Whether the host holds a pending review once this comment is sent; `None` for a task, which
    /// leaves the host's review as it was.
    pub(super) fn leaves_a_pending_review(&self) -> Option<bool> {
        (!self.mode.is_task()).then_some(self.mode == SendMode::AddToReview)
    }

    pub(super) fn sent_comment(&self, from: SentFrom, code: Option<String>) -> SentComment<'_> {
        SentComment {
            place: self.draft.place.describe(),
            from,
            mode: self.mode,
            body: &self.draft.body,
            code,
        }
    }

    /// The draft or queued comment it was sent from is done with.
    pub(super) fn leave(&self, state: &mut ReviewState) {
        let place = &self.draft.place;
        match self.origin {
            Origin::Draft => state.drop_draft(place),
            Origin::Queue => state.drop_queued(place).map(|queued| queued.draft),
        };
    }

    /// A send that failed goes to the queue, or stays in it with the new reason.
    pub(super) fn keep_for_retry(&self, state: &mut ReviewState, reason: &str) {
        let place = &self.draft.place;
        match self.origin {
            Origin::Draft => {
                state.drop_draft(place);
                state.queue_comment(QueuedComment {
                    draft: self.draft.clone(),
                    mode: self.mode,
                    audience: self.audience,
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

    /// A queued comment the host rejected opens as a draft again, so the reviewer can fix it; a
    /// comment from a box stays in its box.
    pub(super) fn keep_as_draft(&self, state: &mut ReviewState) {
        let place = &self.draft.place;
        if self.origin == Origin::Queue {
            state.drop_queued(place);
            state.open_draft(place.clone(), self.draft.written_at.as_ref());
            state.write_draft(place, &self.draft.body);
            for finding in &self.draft.from_findings {
                state.draft_carries(place, finding.clone());
            }
        }
    }

    /// Looks at the PR's head again first: the tour may have fallen behind since the last check.
    /// A local review's comment never reaches the host: it becomes a thread in the state file.
    pub fn run(
        self,
        host: &(impl ChangeTipsPort + ReviewWritesPort + ReviewThreadsPort + ?Sized),
    ) -> CommentOutcome {
        let (how, threads) = match &self.home {
            ThreadHome::Host(pr) => {
                let how = self.send(host, pr);
                let threads = matches!(how, Outcome::Sent).then(|| {
                    host.review_threads(pr)
                        .map_err(|error| format!("{error:#}"))
                });
                (how, threads)
            }
            ThreadHome::StateFile(store) => {
                let how = self.keep_locally(store);
                let threads = matches!(how, Outcome::Sent).then(|| {
                    store
                        .threads_at(&self.tour_head)
                        .map_err(|error| format!("{error:#}"))
                });
                (how, threads)
            }
        };
        CommentOutcome {
            request: self,
            how,
            threads,
        }
    }

    fn keep_locally(&self, store: &LocalThreadStore) -> Outcome {
        if !self.draft.is_placed_at(&self.tour_head) {
            return Outcome::Refused(CommentRefusal::CodeMoved);
        }
        if !self.mode.is_task() {
            return Outcome::Refused(CommentRefusal::NotOffered);
        }
        match store.start(&self.draft.place, &self.draft.body, &self.tour_head) {
            Ok(_) => Outcome::Sent,
            Err(error) => Outcome::Failed(format!("{error:#}")),
        }
    }

    fn send(
        &self,
        host: &(impl ChangeTipsPort + ReviewWritesPort + ?Sized),
        pr: &PullRequestUrl,
    ) -> Outcome {
        if !self.draft.is_placed_at(&self.tour_head) {
            return Outcome::Refused(CommentRefusal::CodeMoved);
        }
        match host.pull_request_tips(pr) {
            Ok(tips) if tips.head == self.tour_head => {}
            Ok(_) => return Outcome::Refused(CommentRefusal::TourIsOld),
            Err(error) => return Outcome::Failed(format!("{error:#}")),
        }
        let sent = match self.mode {
            SendMode::AddToReview => {
                host.add_review_thread(pr, &self.draft.place, &self.draft.body)
            }
            SendMode::CommentNow => match host.has_pending_review(pr) {
                Ok(false) => host.comment_now(pr, &self.draft.place, &self.draft.body),
                Ok(true) => return Outcome::Refused(CommentRefusal::PendingReviewExists),
                Err(error) => return classify(&error),
            },
            SendMode::SendNow | SendMode::AddToTasks => {
                return Outcome::Refused(CommentRefusal::NotOffered);
            }
        };
        sent.map_or_else(|error| classify(&error), |()| Outcome::Sent)
    }
}

fn classify(error: &HostError) -> Outcome {
    match error {
        HostError::ThreadNotCreated(reason) => Outcome::Rejected(reason.clone()),
        HostError::PendingReviewExists => Outcome::Refused(CommentRefusal::PendingReviewExists),
        HostError::NoAccess(no_access) => {
            Outcome::Refused(CommentRefusal::NoAccess(no_access.reason().to_owned()))
        }
        HostError::Other(error) => Outcome::Failed(format!("{error:#}")),
        HostError::NotAnswering(..) | HostError::NotOffered(_) => {
            Outcome::Failed(error.to_string())
        }
    }
}
