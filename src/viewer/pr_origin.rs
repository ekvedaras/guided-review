//! Where a PR comes from and goes to: who opened it, and the branch it merges into.

use std::fmt;
use std::sync::Arc;

use crate::review_subject::BranchName;

/// Who opened the PR. A PR whose author's account is gone has none.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrAuthor {
    pub login: Arc<str>,
    pub avatar_url: Option<Arc<str>>,
}

/// The branch a change comes from: of the repo itself, or of its owner's fork, which GitHub writes
/// `owner:branch`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeadBranch {
    pub fork_owner: Option<Arc<str>>,
    pub branch: BranchName,
}

impl HeadBranch {
    pub const fn of_repo(branch: BranchName) -> Self {
        Self {
            fork_owner: None,
            branch,
        }
    }

    pub fn of_fork(owner: &str, branch: BranchName) -> Self {
        Self {
            fork_owner: Some(owner.into()),
            branch,
        }
    }
}

impl fmt::Display for HeadBranch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.fork_owner {
            Some(owner) => write!(f, "{owner}:{}", self.branch),
            None => write!(f, "{}", self.branch),
        }
    }
}

/// The branch a review's change is compared to and the one it comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergeRoute {
    pub base: BranchName,
    pub head: HeadBranch,
}

/// What GitHub says of a PR's author and branches. The base can be retargeted, so it is read again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PullRequestOrigin {
    pub author: Option<PrAuthor>,
    pub route: MergeRoute,
}
