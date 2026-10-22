use std::fmt;

use crate::pull_request::PullRequestUrl;

use super::ports::{ReviewWritesPort, ViewedSyncUnavailable};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewVerdict {
    Comment,
    Approve,
    RequestChanges,
}

impl ReviewVerdict {
    /// The `PullRequestReviewEvent` value GitHub's GraphQL API uses.
    pub fn github_name(self) -> &'static str {
        match self {
            Self::Comment => "COMMENT",
            Self::Approve => "APPROVE",
            Self::RequestChanges => "REQUEST_CHANGES",
        }
    }
}

/// What approving now would skip over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ApproveWarning {
    pub unread_steps: usize,
    /// Pieces no step shows. Nothing marks them read, so all of them count.
    pub unread_safety_net_pieces: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FinishRefusal {
    /// GitHub does not let anyone approve or request changes on their own PR.
    OwnPullRequest,
    BodyNeeded(ReviewVerdict),
    NoAccess(String),
}

impl fmt::Display for FinishRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OwnPullRequest => {
                f.write_str("You cannot approve or request changes on your own pull request")
            }
            Self::BodyNeeded(ReviewVerdict::RequestChanges) => {
                f.write_str("Say what needs to change")
            }
            Self::BodyNeeded(_) => f.write_str("Write something, or add comments to the review"),
            Self::NoAccess(reason) => write!(f, "No access to GitHub: {reason}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FinishResult {
    Sent,
    Refused(FinishRefusal),
    Failed(String),
}

/// Everything finishing the review needs, owned, so it can run on a background thread.
pub struct FinishRequest {
    pub(super) pr: PullRequestUrl,
    pub(super) verdict: ReviewVerdict,
    pub(super) body: String,
}

pub struct FinishOutcome {
    pub(super) result: FinishResult,
}

impl FinishRequest {
    pub fn run(self, github: &dyn ReviewWritesPort) -> FinishOutcome {
        let result = match github.submit_review(&self.pr, self.verdict, &self.body) {
            Ok(()) => FinishResult::Sent,
            Err(error) => match error.downcast_ref::<ViewedSyncUnavailable>() {
                Some(unavailable) => {
                    FinishResult::Refused(FinishRefusal::NoAccess(unavailable.to_string()))
                }
                None => FinishResult::Failed(format!("{error:#}")),
            },
        };
        FinishOutcome { result }
    }
}
