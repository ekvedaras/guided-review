use std::fmt;

use serde::{Deserialize, Serialize};

use crate::pull_request::{PullRequestUrl, Sha};

use super::ports::{GitHubPort, PendingReviewExists, ThreadNotCreated, ViewedSyncUnavailable};
use super::row_selection::SelectedLines;
use super::step_page::LineLocation;

/// The lines a comment is on: one line, or a range that may cross hunks and sides.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CommentPlace {
    pub start: LineLocation,
    pub end: LineLocation,
}

impl CommentPlace {
    pub fn of(selected: &SelectedLines) -> Self {
        Self {
            start: selected.first().clone(),
            end: selected.last().clone(),
        }
    }

    pub fn path(&self) -> &str {
        &self.end.path
    }

    pub fn is_single_line(&self) -> bool {
        self.start == self.end
    }
}

/// Text the reviewer is writing and has not sent. Kept in the state file on every edit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommentDraft {
    pub place: CommentPlace,
    pub body: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SendMode {
    /// Goes into the pending review, to be sent with the others.
    AddToReview,
    /// Posts at once, as a review of its own.
    CommentNow,
}

/// A comment GitHub did not take yet. It is sent again when GitHub can be reached.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueuedComment {
    pub draft: CommentDraft,
    pub mode: SendMode,
    pub failed_because: String,
}

/// Why a comment was not written, with nothing lost: the text stays where it was.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommentRefusal {
    /// The tour is for an older head, so its lines may not be where GitHub has them.
    TourIsOld,
    NotInsideHunk,
    /// Someone started a pending review since the last look, so "Comment now" is off.
    PendingReviewExists,
    NoAccess(String),
}

impl fmt::Display for CommentRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TourIsOld => f.write_str("Update tour to comment"),
            Self::NotInsideHunk => f.write_str("GitHub takes comments only on lines of the diff"),
            Self::PendingReviewExists => {
                f.write_str("There is a pending review now: add the comment to it")
            }
            Self::NoAccess(reason) => write!(f, "No access to GitHub: {reason}"),
        }
    }
}

/// What happened to a comment the reviewer sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommentResult {
    Sent,
    /// GitHub could not be reached; the comment waits in the queue.
    Queued(String),
    Refused(CommentRefusal),
    /// GitHub answered but made no thread. Retrying would only leave more empty pending reviews,
    /// so the text stays a draft.
    Rejected(String),
}

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
    tour_head: Sha,
}

pub struct CommentOutcome {
    pub(super) request: CommentRequest,
    pub(super) how: Outcome,
}

pub(super) enum Outcome {
    Sent,
    Failed(String),
    Rejected(String),
    Refused(CommentRefusal),
}

impl CommentRequest {
    pub(super) fn new(origin: Origin, draft: CommentDraft, mode: SendMode, tour_head: Sha) -> Self {
        Self {
            origin,
            draft,
            mode,
            tour_head,
        }
    }

    /// Looks at the PR's head again first: the tour may have fallen behind since the last check.
    pub fn run(self, github: &dyn GitHubPort, pr: &PullRequestUrl) -> CommentOutcome {
        let how = self.send(github, pr);
        CommentOutcome { request: self, how }
    }

    fn send(&self, github: &dyn GitHubPort, pr: &PullRequestUrl) -> Outcome {
        match github.pull_request_tips(pr) {
            Ok(tips) if tips.head == self.tour_head => {}
            Ok(_) => return Outcome::Refused(CommentRefusal::TourIsOld),
            Err(error) => return classify(error),
        }
        let sent = match self.mode {
            SendMode::AddToReview => {
                github.add_review_thread(pr, &self.draft.place, &self.draft.body)
            }
            SendMode::CommentNow => match github.has_pending_review(pr) {
                Ok(false) => github.comment_now(pr, &self.draft.place, &self.draft.body),
                Ok(true) => return Outcome::Refused(CommentRefusal::PendingReviewExists),
                Err(error) => return classify(error),
            },
        };
        sent.map_or_else(classify, |()| Outcome::Sent)
    }
}

fn classify(error: anyhow::Error) -> Outcome {
    if error.downcast_ref::<ThreadNotCreated>().is_some() {
        Outcome::Rejected(format!("{error:#}"))
    } else if error.downcast_ref::<PendingReviewExists>().is_some() {
        Outcome::Refused(CommentRefusal::PendingReviewExists)
    } else if let Some(unavailable) = error.downcast_ref::<ViewedSyncUnavailable>() {
        Outcome::Refused(CommentRefusal::NoAccess(unavailable.to_string()))
    } else {
        Outcome::Failed(format!("{error:#}"))
    }
}
