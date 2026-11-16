use crate::pull_request::PullRequestUrl;

use super::ports::PullOfBranch;

/// A PR opened from the branch of a local review, which the review offers to move to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MoveOffer {
    pub pr: PullRequestUrl,
    /// The PR's title, for the PR's tour when it has none yet.
    pub title: String,
}

impl MoveOffer {
    pub(super) fn of(pull: &PullOfBranch) -> Self {
        Self {
            pr: pull.pr.clone(),
            title: pull.title.clone(),
        }
    }

    /// The lasting line's button.
    pub fn label(&self) -> String {
        format!("Move to {}", self.pr.short())
    }
}
