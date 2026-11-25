use super::comments::{CommentPlace, SendMode};
use super::ports::ReviewVerdict;
use super::ports::{
    ChangeTipsPort, ClonePort, CodeHost, HostError, IssueDetailsPort, MergingPort, NoAccess,
    PictureUploadPort, PullOfBranch, PullRequestDescriptionPort, PullRequestOriginPort,
    PullRequestReactionsPort, PullRequestStatusPort, PullsOfBranchPort, RelevantPullRequestsPort,
    Resolution, ReviewThreadsPort, ReviewWritesPort, ViewedFilesPort,
};
use super::pr_origin::PullRequestOrigin;
use super::pr_reviewers::PullRequestReviews;
use super::pr_status::{PrStatus, Readiness};
use super::reactions::{Reaction, ReactionChange};
use super::related_issues::PullRequestIssues;
use super::relevant_prs::RelevantPullRequests;
use super::review_thread::{CommentId, ReviewThread, ThreadId};
use crate::pull_request::{ChangeTips, HostedRepo, PullRequestUrl, RepoName};
use crate::review_subject::BranchName;
use std::path::Path;
use std::sync::Arc;

/// The code host a local review holds: it only looks for the PR opened from its branch, when it was
/// given a way to. It has none of the optional capabilities, so the review offers none of them; a
/// core call, which needs a PR a local review does not have, is refused instead of reaching the network.
#[derive(Clone, Default)]
pub struct NoHost {
    branch_lookups: Option<Arc<dyn PullsOfBranchPort>>,
}

impl NoHost {
    pub fn looking_up_branches_with(lookups: Arc<dyn PullsOfBranchPort>) -> Self {
        Self {
            branch_lookups: Some(lookups),
        }
    }
}

impl PullsOfBranchPort for NoHost {
    fn open_pull_of_branch(
        &self,
        repo: &HostedRepo,
        branch: &BranchName,
    ) -> Result<Option<PullOfBranch>, HostError> {
        match &self.branch_lookups {
            Some(lookups) => lookups.open_pull_of_branch(repo, branch),
            None => refused(),
        }
    }

    fn pull_request_head_branch(
        &self,
        pr: &PullRequestUrl,
    ) -> Result<Option<BranchName>, HostError> {
        match &self.branch_lookups {
            Some(lookups) => lookups.pull_request_head_branch(pr),
            None => refused(),
        }
    }
}

fn refused<T>() -> Result<T, HostError> {
    Err(HostError::NoAccess(NoAccess::new(
        "a local review has no code host",
    )))
}

impl CodeHost for NoHost {
    fn viewed_marks(&self) -> Option<&dyn ViewedFilesPort> {
        None
    }

    fn merging(&self) -> Option<&dyn MergingPort> {
        None
    }

    fn pull_request_reactions(&self) -> Option<&dyn PullRequestReactionsPort> {
        None
    }

    fn picture_uploads(&self) -> Option<&dyn PictureUploadPort> {
        None
    }

    fn issue_tracker(&self) -> Option<&dyn IssueDetailsPort> {
        None
    }
}

impl ClonePort for NoHost {
    fn clone_repo(&self, repo: &RepoName, _folder: &Path) -> anyhow::Result<()> {
        anyhow::bail!("a local review clones no repo, not {repo} either: it reads its own folder")
    }
}

impl RelevantPullRequestsPort for NoHost {
    fn relevant_pull_requests(&self) -> Result<RelevantPullRequests, HostError> {
        refused()
    }
}

impl ChangeTipsPort for NoHost {
    fn pull_request_tips(&self, _pr: &PullRequestUrl) -> Result<ChangeTips, HostError> {
        refused()
    }
}

impl PullRequestDescriptionPort for NoHost {
    fn pull_request_description(&self, _pr: &PullRequestUrl) -> Result<String, HostError> {
        refused()
    }

    fn set_pull_request_description(
        &self,
        _pr: &PullRequestUrl,
        _body: &str,
    ) -> anyhow::Result<()> {
        Ok(refused()?)
    }

    fn pull_request_issues(&self, _pr: &PullRequestUrl) -> Result<PullRequestIssues, HostError> {
        refused()
    }
}

impl PullRequestOriginPort for NoHost {
    fn pull_request_origin(&self, _pr: &PullRequestUrl) -> Result<PullRequestOrigin, HostError> {
        refused()
    }
}

impl PullRequestStatusPort for NoHost {
    fn pull_request_status(&self, _pr: &PullRequestUrl) -> Result<PrStatus, HostError> {
        refused()
    }

    fn set_pull_request_readiness(
        &self,
        _pr: &PullRequestUrl,
        _readiness: Readiness,
    ) -> Result<(), HostError> {
        refused()
    }
}

impl ReviewThreadsPort for NoHost {
    fn review_threads(&self, _pr: &PullRequestUrl) -> Result<Vec<ReviewThread>, HostError> {
        refused()
    }

    fn reply_to_thread(
        &self,
        _pr: &PullRequestUrl,
        _thread_id: &ThreadId,
        _body: &str,
        _mode: SendMode,
    ) -> Result<CommentId, HostError> {
        refused()
    }

    fn set_thread_resolved(
        &self,
        _thread_id: &ThreadId,
        _resolution: Resolution,
    ) -> Result<(), HostError> {
        refused()
    }

    fn set_reaction(
        &self,
        _comment_id: &CommentId,
        _reaction: Reaction,
        _change: ReactionChange,
    ) -> Result<(), HostError> {
        refused()
    }

    fn edit_comment(&self, _comment_id: &CommentId, _body: &str) -> Result<(), HostError> {
        refused()
    }

    fn delete_comment(&self, _comment_id: &CommentId) -> Result<(), HostError> {
        refused()
    }
}

impl ReviewWritesPort for NoHost {
    fn has_pending_review(&self, _pr: &PullRequestUrl) -> Result<bool, HostError> {
        refused()
    }

    fn reviews(&self, _pr: &PullRequestUrl) -> Result<PullRequestReviews, HostError> {
        refused()
    }

    fn viewer_authored(&self, _pr: &PullRequestUrl) -> Result<bool, HostError> {
        refused()
    }

    fn add_review_thread(
        &self,
        _pr: &PullRequestUrl,
        _place: &CommentPlace,
        _body: &str,
    ) -> Result<(), HostError> {
        refused()
    }

    fn comment_now(
        &self,
        _pr: &PullRequestUrl,
        _place: &CommentPlace,
        _body: &str,
    ) -> Result<(), HostError> {
        refused()
    }

    fn submit_review(
        &self,
        _pr: &PullRequestUrl,
        _verdict: ReviewVerdict,
        _body: &str,
    ) -> Result<(), HostError> {
        refused()
    }
}
