use std::fmt;

use crate::pull_request::{PullRequestUrl, Sha};

use super::ports::{GitHubError, ReviewWritesPort};

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

/// How a review that is on GitHub ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewOutcome {
    Approved,
    ChangesRequested,
    Commented,
    /// Someone took the review back; it no longer counts for or against the PR.
    Dismissed,
}

impl SubmittedReview {
    /// Too tall for the sidebar as it is, so it is shown clamped to `CLAMPED_BODY_LINES`.
    pub fn body_is_long(&self) -> bool {
        self.body.lines().count() > CLAMPED_BODY_LINES
            || self.body.chars().count() > LONG_BODY_CHARS
    }

    /// How it ended and when, for a line the reviewer reads; `PR has new commits` when `is_old`.
    pub fn headline(&self, is_old: bool) -> String {
        let outcome = match self.outcome {
            ReviewOutcome::Approved => "Approved",
            ReviewOutcome::ChangesRequested => "Changes requested",
            ReviewOutcome::Commented => "Commented",
            ReviewOutcome::Dismissed => "Dismissed",
        };
        let day = self.submitted_at.split('T').next().unwrap_or_default();
        let since = if is_old {
            ", PR has new commits since"
        } else {
            ""
        };
        format!("Your last review: {outcome} on {day}{since}")
    }
}

impl ReviewOutcome {
    /// From GitHub's `PullRequestReviewState`. A pending review is not sent, so it has no outcome.
    pub fn from_github_state(state: &str) -> Option<Self> {
        match state {
            "APPROVED" => Some(Self::Approved),
            "CHANGES_REQUESTED" => Some(Self::ChangesRequested),
            "COMMENTED" => Some(Self::Commented),
            "DISMISSED" => Some(Self::Dismissed),
            _ => None,
        }
    }
}

pub const CLAMPED_BODY_LINES: usize = 4;
/// About what a sidebar line holds times the clamped lines; a longer text wraps past them.
const LONG_BODY_CHARS: usize = 200;

/// The viewer's latest sent review of a PR, as GitHub holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubmittedReview {
    pub outcome: ReviewOutcome,
    /// What the reviewer wrote with it; empty when nothing.
    pub body: String,
    /// GitHub's UTC time, like `2026-11-05T13:35:33Z`.
    pub submitted_at: String,
    /// The PR's head when the review was sent.
    pub commit: Sha,
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
            Err(GitHubError::NoAccess(no_access)) => {
                FinishResult::Refused(FinishRefusal::NoAccess(no_access.into_reason()))
            }
            Err(error) => FinishResult::Failed(format!("{error:#}")),
        };
        FinishOutcome { result }
    }
}
