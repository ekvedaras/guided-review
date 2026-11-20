//! The findings of the AI review a thread was made from, with Use or Use as is, and what was said about them
//! before: the fix asked for and the Ask why talk.

use std::sync::Arc;

use crate::findings::{FindingId, Severity};

use super::comments::CommentDraft;
use super::finding_threads::WhyMessage;
use super::review_state::ReviewState;
use super::threads::{ReviewThread, ThreadId};

/// A finding a comment of the thread carried.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsedFinding {
    pub id: FindingId,
    pub severity: Severity,
    /// Where the fix the reviewer asked for got to, when one was asked for.
    pub fix: Option<Arc<str>>,
    /// Ask why and fix requests on it, oldest first.
    pub talk: Arc<[WhyMessage]>,
}

/// Reads, for each thread, the findings its comment was made from.
#[derive(Clone, Copy)]
pub(super) struct FindingsOfThreads<'a> {
    state: &'a ReviewState,
}

impl<'a> FindingsOfThreads<'a> {
    pub const fn of(state: &'a ReviewState) -> Self {
        Self { state }
    }

    pub fn of_thread(self, thread: &ThreadId) -> Vec<UsedFinding> {
        self.state
            .finding_decisions()
            .used_in(thread)
            .map(|finding| UsedFinding {
                id: finding.id.clone(),
                severity: finding.severity,
                fix: self
                    .state
                    .fix_requests()
                    .state_of(&finding.id)
                    .map(|state| state.line().into()),
                talk: self
                    .state
                    .finding_threads()
                    .thread_of(&finding.id)
                    .map_or_else(|| Arc::from([]), |talk| talk.messages().into()),
            })
            .collect()
    }
}

/// The thread a sent comment became: one not known before it was sent, on its file, holding its text.
pub(super) fn thread_made_of(
    draft: &CommentDraft,
    threads: Option<&Result<Vec<ReviewThread>, String>>,
    known_before: &[ReviewThread],
) -> Option<ThreadId> {
    let body = draft.body.trim();
    threads?
        .as_ref()
        .ok()?
        .iter()
        .find(|thread| {
            !known_before.iter().any(|known| known.id == thread.id)
                && thread.path == *draft.place.path()
                && thread
                    .comments
                    .iter()
                    .any(|comment| comment.body.trim_end().ends_with(body))
        })
        .map(|thread| thread.id.clone())
}
