use std::path::Path;

use super::comments::{CommentPlace, SendMode};
use super::merging::MergeChange;
use super::ports::ReviewVerdict;
use super::ports::{
    ChangeTipsPort, HostError, IssueDetailsPort, NoAccess, PictureUploadPort, PullOfBranch,
    PullRequestDescriptionPort, PullRequestOriginPort, PullRequestReactionsPort,
    PullRequestStatusPort, PullsOfBranchPort, Resolution, ReviewThreadsPort, ReviewWritesPort,
    Viewed, ViewedFile, ViewedFilesPort,
};
use super::pr_origin::PullRequestOrigin;
use super::pr_reviewers::PullRequestReviews;
use super::pr_status::{PrStatus, Readiness};
use super::reactions::{Reaction, ReactionChange};
use super::related_issues::{IssueDetails, PullRequestIssues};
use super::review_thread::{CommentId, ReviewThread, ThreadId};
use crate::file_path::FilePath;
use crate::issue::IssueUrl;
use crate::pull_request::{ChangeTips, PullRequestUrl, RepoName};
use crate::review_subject::BranchName;
use std::sync::Arc;

/// The code host a local review holds: it only looks for the PR opened from its branch, when it was
/// given a way to. Anything else a review did would be refused instead of reaching the network.
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
        repo: &RepoName,
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
        "a local review has no GitHub",
    )))
}

impl ChangeTipsPort for NoHost {
    fn pull_request_tips(&self, _pr: &PullRequestUrl) -> Result<ChangeTips, HostError> {
        refused()
    }
}

impl ViewedFilesPort for NoHost {
    fn viewed_files(&self, _pr: &PullRequestUrl) -> Result<Vec<ViewedFile>, HostError> {
        refused()
    }

    fn set_file_viewed(
        &self,
        _pr: &PullRequestUrl,
        _path: &FilePath,
        _viewed: Viewed,
    ) -> Result<(), HostError> {
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

impl IssueDetailsPort for NoHost {
    fn issue_details(
        &self,
        _issues: &[IssueUrl],
    ) -> Result<Vec<(IssueUrl, IssueDetails)>, HostError> {
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

    fn change_merging(&self, _pr: &PullRequestUrl, _change: MergeChange) -> Result<(), HostError> {
        refused()
    }
}

impl PullRequestReactionsPort for NoHost {
    fn set_pull_request_reaction(
        &self,
        _pr: &PullRequestUrl,
        _reaction: Reaction,
        _change: ReactionChange,
    ) -> Result<(), HostError> {
        refused()
    }
}

impl PictureUploadPort for NoHost {
    fn upload_picture(&self, _pr: &PullRequestUrl, _picture: &Path) -> anyhow::Result<String> {
        Ok(refused()?)
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
