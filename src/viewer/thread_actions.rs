use crate::pull_request::PullRequestUrl;

use super::comment_desk::TourFreshness;
use super::comments::{CommentRefusal, SendMode};
use super::ports::{GitHubError, Resolution, ReviewThreadsPort, ReviewWritesPort};
use super::reactions::{Reaction, ReactionChange};
use super::threads::{CommentId, ReviewThread, ThreadId};

/// What the reviewer does to a thread that is already on GitHub.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThreadAction {
    /// Sends the thread's reply draft as it is saved when the request is made, so a key typed
    /// just before sending is in it.
    Reply {
        thread_id: ThreadId,
        mode: SendMode,
    },
    Resolve {
        thread_id: ThreadId,
    },
    Unresolve {
        thread_id: ThreadId,
    },
    /// Replaces the comment's text with its edit draft as it is saved when the request is made.
    EditComment {
        comment_id: CommentId,
    },
    /// Adds the viewer's reaction to a comment, or takes it back.
    SetReaction {
        comment_id: CommentId,
        reaction: Reaction,
        change: ReactionChange,
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
    /// The reply draft of the thread a `Reply` is for, or the edit draft of the comment an
    /// `EditComment` is for.
    pub(super) draft_body: Option<String>,
    /// The tour is behind the PR head: the thread may sit on lines the tour does not show.
    pub(super) freshness: TourFreshness,
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
            Ok(result) => result,
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
    ) -> Result<ThreadActionResult, GitHubError> {
        match &self.action {
            ThreadAction::Reply { .. } if self.freshness == TourFreshness::Behind => {
                return Ok(ThreadActionResult::Refused(CommentRefusal::TourIsOld));
            }
            ThreadAction::Reply { thread_id, mode } => {
                let Some(body) = self
                    .draft_body
                    .as_deref()
                    .filter(|body| !body.trim().is_empty())
                else {
                    return Ok(ThreadActionResult::Refused(CommentRefusal::EmptyReply));
                };
                if *mode == SendMode::CommentNow && github.has_pending_review(&self.pr)? {
                    return Ok(ThreadActionResult::Refused(
                        CommentRefusal::PendingReviewExists,
                    ));
                }
                github.reply_to_thread(&self.pr, thread_id, body, *mode)?;
            }
            ThreadAction::EditComment { comment_id } => {
                let Some(body) = self
                    .draft_body
                    .as_deref()
                    .filter(|body| !body.trim().is_empty())
                else {
                    return Ok(ThreadActionResult::Refused(CommentRefusal::EmptyComment));
                };
                github.edit_comment(comment_id, body)?;
            }
            ThreadAction::Resolve { thread_id } => {
                github.set_thread_resolved(thread_id, Resolution::Resolved)?;
            }
            ThreadAction::Unresolve { thread_id } => {
                github.set_thread_resolved(thread_id, Resolution::Unresolved)?;
            }
            ThreadAction::SetReaction {
                comment_id,
                reaction,
                change,
            } => github.set_reaction(comment_id, *reaction, *change)?,
        }
        Ok(ThreadActionResult::Done)
    }
}

fn classify(error: &GitHubError) -> ThreadActionResult {
    match error {
        GitHubError::PendingReviewExists => {
            ThreadActionResult::Refused(CommentRefusal::PendingReviewExists)
        }
        GitHubError::NoAccess(no_access) => {
            ThreadActionResult::Refused(CommentRefusal::NoAccess(no_access.reason().to_owned()))
        }
        GitHubError::ThreadNotCreated(_) | GitHubError::Other(_) => {
            ThreadActionResult::Failed(format!("{error:#}"))
        }
    }
}
