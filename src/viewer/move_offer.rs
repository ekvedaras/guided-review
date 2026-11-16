use crate::pull_request::PullRequestUrl;
use crate::review_subject::LocalChangeSet;

use super::ports::PullOfBranch;

/// A PR opened from the branch of a local review, which the review offers to move to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MoveOffer {
    pub pr: PullRequestUrl,
    /// The PR's title, for the PR's tour when it has none yet.
    pub title: String,
}

/// A local review of the branch a PR is opened from, which the PR's review offers to take over.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MoveHereOffer {
    pub change_set: LocalChangeSet,
    pub offer: MoveOffer,
}

impl MoveHereOffer {
    /// The lasting line's button.
    pub fn label(&self) -> String {
        format!("Move {} here", self.change_set.branch().as_str())
    }
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
