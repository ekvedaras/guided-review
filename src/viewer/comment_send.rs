//! Sending one comment: to GitHub, or into a local review's state file, on a background thread.

use super::comments::{Audience, CommentDraft, CommentRefusal, SendMode};
use super::local_thread_store::{LocalThreadStore, ThreadHome};
use super::ports::{ChangeTipsPort, GitHubError, ReviewThreadsPort, ReviewWritesPort};
use super::review_thread::ReviewThread;
use crate::pull_request::{PullRequestUrl, Sha};
use crate::viewer::comments::CommentPlace;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Origin {
    Draft,
    Queue,
}

/// Everything sending one comment needs, owned, so it can run on a background thread.
pub struct CommentRequest {
    pub(super) origin: Origin,
    pub(super) draft: CommentDraft,
    pub(super) mode: SendMode,
    pub(super) audience: Audience,
    tour_head: Sha,
    home: ThreadHome,
}

pub struct CommentOutcome {
    pub(super) request: CommentRequest,
    pub(super) how: Outcome,
    /// The threads as GitHub has them after the comment went in, read in the same background run,
    /// so the comment box is replaced by its thread in one step; `None` when nothing was sent.
    pub(super) threads: Option<Result<Vec<ReviewThread>, String>>,
}

pub(super) enum Outcome {
    Sent,
    Failed(String),
    Rejected(String),
    Refused(CommentRefusal),
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

    /// Looks at the PR's head again first: the tour may have fallen behind since the last check.
    /// A local review's comment never reaches GitHub: it becomes a thread in the state file.
    pub fn run(
        self,
        github: &(impl ChangeTipsPort + ReviewWritesPort + ReviewThreadsPort + ?Sized),
    ) -> CommentOutcome {
        let (how, threads) = match &self.home {
            ThreadHome::GitHub(pr) => {
                let how = self.send(github, pr);
                let threads = matches!(how, Outcome::Sent).then(|| {
                    github
                        .review_threads(pr)
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
        github: &(impl ChangeTipsPort + ReviewWritesPort + ?Sized),
        pr: &PullRequestUrl,
    ) -> Outcome {
        if !self.draft.is_placed_at(&self.tour_head) {
            return Outcome::Refused(CommentRefusal::CodeMoved);
        }
        match github.pull_request_tips(pr) {
            Ok(tips) if tips.head == self.tour_head => {}
            Ok(_) => return Outcome::Refused(CommentRefusal::TourIsOld),
            Err(error) => return Outcome::Failed(format!("{error:#}")),
        }
        let sent = match self.mode {
            SendMode::AddToReview => {
                github.add_review_thread(pr, &self.draft.place, &self.draft.body)
            }
            SendMode::CommentNow => match github.has_pending_review(pr) {
                Ok(false) => github.comment_now(pr, &self.draft.place, &self.draft.body),
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

fn classify(error: &GitHubError) -> Outcome {
    match error {
        GitHubError::ThreadNotCreated(reason) => Outcome::Rejected(reason.clone()),
        GitHubError::PendingReviewExists => Outcome::Refused(CommentRefusal::PendingReviewExists),
        GitHubError::NoAccess(no_access) => {
            Outcome::Refused(CommentRefusal::NoAccess(no_access.reason().to_owned()))
        }
        GitHubError::Other(error) => Outcome::Failed(format!("{error:#}")),
        GitHubError::NotAnswering(_) => Outcome::Failed(error.to_string()),
    }
}
