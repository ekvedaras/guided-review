use crate::pull_request::{PullRequestUrl, Sha};

use super::local_thread_store::{LocalThreadStore, ThreadHome, ThreadHomes};
use super::local_threads::LocalAuthor;

use super::capability::Capability;
use super::comments::Audience;
use super::comments::{CommentRefusal, SendMode};
use super::ports::{CodeHost, HostError, Resolution};
use super::reactions::{Reaction, ReactionChange};
use super::review_thread::{CommentId, ReviewThread, ThreadId};

/// What the reviewer does to a thread that is already on the host.
impl ThreadAction {
    /// Where the thread or comment the action is about is kept.
    pub(super) fn home_in(&self, homes: &ThreadHomes) -> ThreadHome {
        match self {
            Self::Reply { thread_id, .. }
            | Self::Resolve { thread_id }
            | Self::Unresolve { thread_id } => homes.of_thread(thread_id),
            Self::EditComment { comment_id }
            | Self::DeleteComment { comment_id }
            | Self::SetReaction { comment_id, .. } => homes.of_comment(comment_id),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThreadAction {
    /// Sends the thread's reply draft as it is saved when the request is made, so a key typed
    /// just before sending is in it.
    Reply {
        thread_id: ThreadId,
        mode: SendMode,
        /// Comment and ask agent hands the reply to the tour agent too.
        audience: Audience,
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
    /// Removes the viewer's own comment, once the reviewer has said yes to the question.
    DeleteComment {
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
    /// The host could not do it. Nothing changed, so the reviewer can try again.
    Failed(String),
}

/// Everything one thread action needs, owned, so it can run on a background thread.
pub struct ThreadActionRequest {
    pub(super) home: ThreadHome,
    /// The head the review reads, which a local thread's lines are seen from.
    pub(super) head: Sha,
    pub(super) action: ThreadAction,
    /// The reply draft of the thread a `Reply` is for, or the edit draft of the comment an
    /// `EditComment` is for.
    pub(super) draft_body: Option<String>,
    /// The tour is behind the PR head: the thread may sit on lines the tour does not show.
    pub(super) freshness: TourFreshness,
    /// What the review already knows stops the action, so it never reaches the host.
    pub(super) refusal: Option<CommentRefusal>,
}

pub struct ThreadActionOutcome {
    pub(super) action: ThreadAction,
    /// Where the thread the action was about is kept, so its threads are the ones to read again.
    pub(super) home: ThreadHome,
    pub(super) result: ThreadActionResult,
    /// The threads as the host has them after the action, so the screen shows what the host shows;
    /// `None` when the action did not go through, so nothing changed.
    pub(super) threads: Option<Result<Vec<ReviewThread>, String>>,
    /// The text of the reply a `Reply` sent.
    pub(super) reply_body: Option<String>,
}

impl ThreadActionRequest {
    pub fn run(self, host: &dyn CodeHost) -> ThreadActionOutcome {
        let result = match &self.home {
            ThreadHome::Host(pr) => match self.perform(host, pr) {
                Ok(result) => result,
                Err(error) => classify(&error),
            },
            ThreadHome::StateFile(store) => self
                .perform_locally(store)
                .unwrap_or_else(|error| ThreadActionResult::Failed(format!("{error:#}"))),
        };
        let threads = (result == ThreadActionResult::Done).then(|| match &self.home {
            ThreadHome::Host(pr) => host
                .review_threads(pr)
                .map_err(|error| format!("{error:#}")),
            ThreadHome::StateFile(store) => store
                .threads_at(&self.head)
                .map_err(|error| format!("{error:#}")),
        });
        let reply_body = matches!(self.action, ThreadAction::Reply { .. })
            .then_some(self.draft_body)
            .flatten();
        ThreadActionOutcome {
            action: self.action,
            home: self.home,
            result,
            threads,
            reply_body,
        }
    }

    /// What the review knows already, or the text, stops the action before it goes anywhere.
    fn refusal_before_sending(&self) -> Option<CommentRefusal> {
        if let Some(refusal) = &self.refusal {
            return Some(refusal.clone());
        }
        match &self.action {
            ThreadAction::Reply { .. } if self.freshness == TourFreshness::Behind => {
                Some(CommentRefusal::TourIsOld)
            }
            ThreadAction::Reply { .. } if self.text().is_none() => Some(CommentRefusal::EmptyReply),
            ThreadAction::EditComment { .. } if self.text().is_none() => {
                Some(CommentRefusal::EmptyComment)
            }
            _ => None,
        }
    }

    fn text(&self) -> Option<&str> {
        self.draft_body
            .as_deref()
            .filter(|body| !body.trim().is_empty())
    }

    fn perform_locally(&self, store: &LocalThreadStore) -> anyhow::Result<ThreadActionResult> {
        if let Some(refusal) = self.refusal_before_sending() {
            return Ok(ThreadActionResult::Refused(refusal));
        }
        let text = self.text().unwrap_or_default();
        match &self.action {
            ThreadAction::Reply { mode, .. } if !mode.is_task() => {
                return Ok(ThreadActionResult::Refused(CommentRefusal::NotOffered));
            }
            ThreadAction::Reply { thread_id, .. } => {
                store.reply(thread_id, text, LocalAuthor::Reviewer)?;
            }
            ThreadAction::EditComment { comment_id } => store.edit(comment_id, text)?,
            ThreadAction::DeleteComment { comment_id } => store.delete(comment_id)?,
            ThreadAction::Resolve { thread_id } => {
                store.set_resolved(thread_id, Resolution::Resolved)?;
            }
            ThreadAction::Unresolve { thread_id } => {
                store.set_resolved(thread_id, Resolution::Unresolved)?;
            }
            ThreadAction::SetReaction { .. } => {
                return Ok(ThreadActionResult::Refused(CommentRefusal::NotOffered));
            }
        }
        Ok(ThreadActionResult::Done)
    }

    fn perform(
        &self,
        host: &dyn CodeHost,
        pr: &PullRequestUrl,
    ) -> Result<ThreadActionResult, HostError> {
        if let Some(refusal) = self.refusal_before_sending() {
            return Ok(ThreadActionResult::Refused(refusal));
        }
        let text = self.text().unwrap_or_default();
        match &self.action {
            ThreadAction::Reply { mode, .. } if mode.is_task() => {
                return Ok(ThreadActionResult::Refused(CommentRefusal::NotOffered));
            }
            ThreadAction::Reply {
                thread_id, mode, ..
            } => {
                if *mode == SendMode::CommentNow && host.has_pending_review(pr)? {
                    return Ok(ThreadActionResult::Refused(
                        CommentRefusal::PendingReviewExists,
                    ));
                }
                host.reply_to_thread(pr, thread_id, text, *mode)?;
            }
            ThreadAction::EditComment { comment_id } => host.edit_comment(comment_id, text)?,
            ThreadAction::DeleteComment { comment_id } => host.delete_comment(comment_id)?,
            ThreadAction::Resolve { thread_id } => {
                host.set_thread_resolved(thread_id, Resolution::Resolved)?;
            }
            ThreadAction::Unresolve { thread_id } => {
                host.set_thread_resolved(thread_id, Resolution::Unresolved)?;
            }
            ThreadAction::SetReaction {
                comment_id,
                reaction,
                change,
            } => match host.comment_reactions() {
                Some(reactions) => {
                    reactions.set_comment_reaction(comment_id, *reaction, *change)?;
                }
                None => return Err(HostError::NotOffered(Capability::CommentReactions)),
            },
        }
        Ok(ThreadActionResult::Done)
    }
}

fn classify(error: &HostError) -> ThreadActionResult {
    match error {
        HostError::PendingReviewExists => {
            ThreadActionResult::Refused(CommentRefusal::PendingReviewExists)
        }
        HostError::NoAccess(no_access) => {
            ThreadActionResult::Refused(CommentRefusal::NoAccess(no_access.reason().to_owned()))
        }
        HostError::ThreadNotCreated(_)
        | HostError::NotAnswering(..)
        | HostError::NotOffered(_)
        | HostError::Refused(_)
        | HostError::Other(_) => ThreadActionResult::Failed(format!("{error:#}")),
    }
}

/// Whether the tour shows the PR as it is now: a comment on an older tour may land on lines the tour does not show.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TourFreshness {
    Current,
    Behind,
}
