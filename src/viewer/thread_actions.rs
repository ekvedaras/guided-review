use crate::pull_request::PullRequestUrl;

use super::comments::{CommentRefusal, SendMode};
use super::ports::{GitHubPort, PendingReviewExists, ViewedSyncUnavailable};
use super::reactions::Reaction;
use super::threads::ReviewThread;

/// What the reviewer does to a thread that is already on GitHub.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThreadAction {
    Reply {
        thread_id: String,
        body: String,
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
    /// The tour is behind the PR head: the thread may sit on lines the tour does not show.
    pub(super) tour_is_old: bool,
}

pub struct ThreadActionOutcome {
    pub(super) action: ThreadAction,
    pub(super) result: ThreadActionResult,
    /// The threads as GitHub has them after the action, so the screen shows what GitHub shows.
    pub(super) threads: Option<Vec<ReviewThread>>,
}

impl ThreadActionRequest {
    pub fn run(self, github: &dyn GitHubPort) -> ThreadActionOutcome {
        let result = match self.perform(github) {
            Ok(()) => ThreadActionResult::Done,
            Err(error) => classify(error),
        };
        let threads = (result == ThreadActionResult::Done)
            .then(|| github.review_threads(&self.pr).ok())
            .flatten();
        ThreadActionOutcome {
            action: self.action,
            result,
            threads,
        }
    }

    fn perform(&self, github: &dyn GitHubPort) -> anyhow::Result<()> {
        if self.tour_is_old && matches!(self.action, ThreadAction::Reply { .. }) {
            return Err(TourIsOld.into());
        }
        match &self.action {
            ThreadAction::Reply {
                thread_id,
                body,
                mode,
            } => {
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

fn classify(error: anyhow::Error) -> ThreadActionResult {
    if error.downcast_ref::<TourIsOld>().is_some() {
        ThreadActionResult::Refused(CommentRefusal::TourIsOld)
    } else if error.downcast_ref::<PendingReviewExists>().is_some() {
        ThreadActionResult::Refused(CommentRefusal::PendingReviewExists)
    } else if let Some(unavailable) = error.downcast_ref::<ViewedSyncUnavailable>() {
        ThreadActionResult::Refused(CommentRefusal::NoAccess(unavailable.to_string()))
    } else {
        ThreadActionResult::Failed(format!("{error:#}"))
    }
}
