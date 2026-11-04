use crate::pull_request::PullRequestUrl;

use super::comments::{CommentRefusal, SendMode};
use super::ports::{
    PendingReviewExists, ReviewThreadsPort, ReviewWritesPort, ViewedSyncUnavailable,
};
use super::reactions::Reaction;
use super::threads::ReviewThread;

/// What the reviewer does to a thread that is already on GitHub.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThreadAction {
    /// Sends the thread's reply draft as it is saved when the request is made, so a key typed
    /// just before sending is in it.
    Reply {
        thread_id: String,
        mode: SendMode,
    },
    Resolve {
        thread_id: String,
    },
    Unresolve {
        thread_id: String,
    },
    /// Adds the viewer's reaction to a comment, or takes it back.
    SetReaction {
        comment_id: String,
        reaction: Reaction,
        on: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThreadActionResult {
    Done,
    Refused(CommentRefusal),
    /// GitHub could not do it. Nothing changed, so the reviewer can try again.
    Failed(String),
}

/// Everything one thread action needs, owned, so it can run on a background thread.
pub struct ThreadActionRequest {
    pub(super) pr: PullRequestUrl,
    pub(super) action: ThreadAction,
    /// The reply draft of the thread a `Reply` is for.
    pub(super) reply_body: Option<String>,
    /// The tour is behind the PR head: the thread may sit on lines the tour does not show.
    pub(super) tour_is_old: bool,
}

pub struct ThreadActionOutcome {
    pub(super) action: ThreadAction,
    pub(super) result: ThreadActionResult,
    /// The threads as GitHub has them after the action, so the screen shows what GitHub shows;
    /// `None` when the action did not go through, so nothing changed.
    pub(super) threads: Option<Result<Vec<ReviewThread>, String>>,
}

impl ThreadActionRequest {
    pub fn run(
        self,
        github: &(impl ReviewThreadsPort + ReviewWritesPort + ?Sized),
    ) -> ThreadActionOutcome {
        let result = match self.perform(github) {
            Ok(()) => ThreadActionResult::Done,
            Err(error) => classify(&error),
        };
        let threads = (result == ThreadActionResult::Done).then(|| {
            github
                .review_threads(&self.pr)
                .map_err(|error| format!("{error:#}"))
        });
        ThreadActionOutcome {
            action: self.action,
            result,
            threads,
        }
    }

    fn perform(
        &self,
        github: &(impl ReviewThreadsPort + ReviewWritesPort + ?Sized),
    ) -> anyhow::Result<()> {
        if self.tour_is_old && matches!(self.action, ThreadAction::Reply { .. }) {
            return Err(TourIsOld.into());
        }
        match &self.action {
            ThreadAction::Reply { thread_id, mode } => {
                let body = self
                    .reply_body
                    .as_deref()
                    .filter(|body| !body.trim().is_empty())
                    .ok_or(EmptyReply)?;
                if *mode == SendMode::CommentNow && github.has_pending_review(&self.pr)? {
                    return Err(PendingReviewExists.into());
                }
                github.reply_to_thread(&self.pr, thread_id, body, *mode)
            }
            ThreadAction::Resolve { thread_id } => github.set_thread_resolved(thread_id, true),
            ThreadAction::Unresolve { thread_id } => github.set_thread_resolved(thread_id, false),
            ThreadAction::SetReaction {
                comment_id,
                reaction,
                on,
            } => github.set_reaction(comment_id, *reaction, *on),
        }
    }
}

#[derive(Debug)]
struct TourIsOld;

impl std::fmt::Display for TourIsOld {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        CommentRefusal::TourIsOld.fmt(f)
    }
}

impl std::error::Error for TourIsOld {}

#[derive(Debug)]
struct EmptyReply;

impl std::fmt::Display for EmptyReply {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        CommentRefusal::EmptyReply.fmt(f)
    }
}

impl std::error::Error for EmptyReply {}

fn classify(error: &anyhow::Error) -> ThreadActionResult {
    if error.downcast_ref::<TourIsOld>().is_some() {
        ThreadActionResult::Refused(CommentRefusal::TourIsOld)
    } else if error.downcast_ref::<EmptyReply>().is_some() {
        ThreadActionResult::Refused(CommentRefusal::EmptyReply)
    } else if error.downcast_ref::<PendingReviewExists>().is_some() {
        ThreadActionResult::Refused(CommentRefusal::PendingReviewExists)
    } else if let Some(unavailable) = error.downcast_ref::<ViewedSyncUnavailable>() {
        ThreadActionResult::Refused(CommentRefusal::NoAccess(unavailable.to_string()))
    } else {
        ThreadActionResult::Failed(format!("{error:#}"))
    }
}
