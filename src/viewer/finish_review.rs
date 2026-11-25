use std::fmt;

use crate::pull_request::PullRequestUrl;

use super::capability::Capability;
use super::ports::{CodeHost, HostError};

/// What the reviewer's review says of the PR besides its comments.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewVerdict {
    Comment,
    Approve,
    /// Only on a host that has requests for changes (`Capability::ChangeRequests`).
    RequestChanges,
}

/// What finishing does: a PR takes a review on GitHub; a local review sends its queued tasks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FinishAction {
    FinishReview,
    SendTasks,
    /// A re-review with nothing new to send ends on the reviewer's say-so, not on GitHub.
    MarkDone,
}

/// What approving now would skip over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ApproveWarning {
    pub unread_steps: usize,
    /// A basic tour's unread files; a guided tour has none.
    pub unread_files: usize,
    /// Pieces no step shows. Nothing marks them read, so all of them count.
    pub unread_safety_net_pieces: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FinishRefusal {
    /// GitHub does not let anyone approve or request changes on their own PR.
    OwnPullRequest,
    BodyNeeded(ReviewVerdict),
    NoAccess(String),
    /// A local review has no PR to send a review to.
    NoPullRequest,
    /// The PR's host has no such verdict.
    NotOffered(Capability),
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
            Self::NoPullRequest => f.write_str("A local review has no pull request to send to"),
            Self::NotOffered(capability) => {
                write!(f, "The pull request's host has no {capability}")
            }
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
    pub fn run(self, host: &dyn CodeHost) -> FinishOutcome {
        let (pr, body) = (&self.pr, self.body.as_str());
        let sent = match self.verdict {
            ReviewVerdict::Comment => host.submit_review(pr, body),
            ReviewVerdict::Approve => host.approve(pr, body),
            ReviewVerdict::RequestChanges => match host.change_requests() {
                Some(change_requests) => change_requests.request_changes(pr, body),
                None => Err(HostError::NotOffered(Capability::ChangeRequests)),
            },
        };
        let result = match sent {
            Ok(()) => FinishResult::Sent,
            Err(HostError::NoAccess(no_access)) => {
                FinishResult::Refused(FinishRefusal::NoAccess(no_access.into_reason()))
            }
            Err(error) => FinishResult::Failed(format!("{error:#}")),
        };
        FinishOutcome { result }
    }
}
