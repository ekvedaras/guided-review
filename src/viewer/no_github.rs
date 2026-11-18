use std::path::Path;

use super::comments::{CommentPlace, SendMode};
use super::finish_review::{ReviewVerdict, SubmittedReview};
use super::merging::MergeChange;
use super::ports::{
    ChangeTipsPort, GitHubError, IssueDetailsPort, NoAccess, PictureUploadPort, PullOfBranch,
    PullRequestDescriptionPort, PullRequestReactionsPort, PullRequestStatusPort, PullsOfBranchPort,
    Resolution, ReviewThreadsPort, ReviewWritesPort, Viewed, ViewedFile, ViewedFilesPort,
};
use super::pr_status::{PrStatus, Readiness};
use super::reactions::{Reaction, ReactionChange};
use super::related_issues::{IssueDetails, PullRequestIssues};
use super::threads::{CommentId, ReviewThread, ThreadId};
use crate::file_path::FilePath;
use crate::issue::IssueUrl;
use crate::pull_request::{ChangeTips, PullRequestUrl, RepoName};
use crate::review_subject::BranchName;
use std::sync::Arc;

/// The GitHub a local review holds: it only looks for the PR opened from its branch, when it was given
/// a way to. Anything else a review did would be refused instead of reaching the network.
#[derive(Clone, Default)]
pub struct NoGitHub {
    branch_lookups: Option<Arc<dyn PullsOfBranchPort>>,
}

impl NoGitHub {
    pub fn looking_up_branches_with(lookups: Arc<dyn PullsOfBranchPort>) -> Self {
        Self {
            branch_lookups: Some(lookups),
        }
    }
}

impl PullsOfBranchPort for NoGitHub {
    fn open_pull_of_branch(
        &self,
        repo: &RepoName,
        branch: &BranchName,
    ) -> Result<Option<PullOfBranch>, GitHubError> {
        match &self.branch_lookups {
            Some(lookups) => lookups.open_pull_of_branch(repo, branch),
            None => refused(),
        }
    }

    fn pull_request_head_branch(
        &self,
        pr: &PullRequestUrl,
    ) -> Result<Option<BranchName>, GitHubError> {
        match &self.branch_lookups {
            Some(lookups) => lookups.pull_request_head_branch(pr),
            None => refused(),
        }
    }
}

fn refused<T>() -> Result<T, GitHubError> {
    Err(GitHubError::NoAccess(NoAccess::new(
        "a local review has no GitHub",
    )))
}

impl ChangeTipsPort for NoGitHub {
    fn pull_request_tips(&self, _pr: &PullRequestUrl) -> Result<ChangeTips, GitHubError> {
        refused()
    }
}

impl ViewedFilesPort for NoGitHub {
    fn viewed_files(&self, _pr: &PullRequestUrl) -> Result<Vec<ViewedFile>, GitHubError> {
        refused()
    }

    fn set_file_viewed(
        &self,
        _pr: &PullRequestUrl,
        _path: &FilePath,
        _viewed: Viewed,
    ) -> Result<(), GitHubError> {
        refused()
    }
}

impl PullRequestDescriptionPort for NoGitHub {
    fn pull_request_description(&self, _pr: &PullRequestUrl) -> Result<String, GitHubError> {
        refused()
    }

    fn set_pull_request_description(
        &self,
        _pr: &PullRequestUrl,
        _body: &str,
    ) -> anyhow::Result<()> {
        Ok(refused()?)
    }

    fn pull_request_issues(&self, _pr: &PullRequestUrl) -> Result<PullRequestIssues, GitHubError> {
        refused()
    }
}

impl IssueDetailsPort for NoGitHub {
    fn issue_details(
        &self,
        _issues: &[IssueUrl],
    ) -> Result<Vec<(IssueUrl, IssueDetails)>, GitHubError> {
        refused()
    }
}

impl PullRequestStatusPort for NoGitHub {
    fn pull_request_status(&self, _pr: &PullRequestUrl) -> Result<PrStatus, GitHubError> {
        refused()
    }

    fn set_pull_request_readiness(
        &self,
        _pr: &PullRequestUrl,
        _readiness: Readiness,
    ) -> Result<(), GitHubError> {
        refused()
    }

    fn change_merging(
        &self,
        _pr: &PullRequestUrl,
        _change: MergeChange,
    ) -> Result<(), GitHubError> {
        refused()
    }
}

impl PullRequestReactionsPort for NoGitHub {
    fn set_pull_request_reaction(
        &self,
        _pr: &PullRequestUrl,
        _reaction: Reaction,
        _change: ReactionChange,
    ) -> Result<(), GitHubError> {
        refused()
    }
}

impl PictureUploadPort for NoGitHub {
    fn upload_picture(&self, _pr: &PullRequestUrl, _picture: &Path) -> anyhow::Result<String> {
        Ok(refused()?)
    }
}

impl ReviewThreadsPort for NoGitHub {
    fn review_threads(&self, _pr: &PullRequestUrl) -> Result<Vec<ReviewThread>, GitHubError> {
        refused()
    }

    fn reply_to_thread(
        &self,
        _pr: &PullRequestUrl,
        _thread_id: &ThreadId,
        _body: &str,
        _mode: SendMode,
    ) -> Result<CommentId, GitHubError> {
        refused()
    }

    fn set_thread_resolved(
        &self,
        _thread_id: &ThreadId,
        _resolution: Resolution,
    ) -> Result<(), GitHubError> {
        refused()
    }

    fn set_reaction(
        &self,
        _comment_id: &CommentId,
        _reaction: Reaction,
        _change: ReactionChange,
    ) -> Result<(), GitHubError> {
        refused()
    }

    fn edit_comment(&self, _comment_id: &CommentId, _body: &str) -> Result<(), GitHubError> {
        refused()
    }

    fn delete_comment(&self, _comment_id: &CommentId) -> Result<(), GitHubError> {
        refused()
    }
}

impl ReviewWritesPort for NoGitHub {
    fn has_pending_review(&self, _pr: &PullRequestUrl) -> Result<bool, GitHubError> {
        refused()
    }

    fn viewer_latest_review(
        &self,
        _pr: &PullRequestUrl,
    ) -> Result<Option<SubmittedReview>, GitHubError> {
        refused()
    }

    fn viewer_authored(&self, _pr: &PullRequestUrl) -> Result<bool, GitHubError> {
        refused()
    }

    fn add_review_thread(
        &self,
        _pr: &PullRequestUrl,
        _place: &CommentPlace,
        _body: &str,
    ) -> Result<(), GitHubError> {
        refused()
    }

    fn comment_now(
        &self,
        _pr: &PullRequestUrl,
        _place: &CommentPlace,
        _body: &str,
    ) -> Result<(), GitHubError> {
        refused()
    }

    fn submit_review(
        &self,
        _pr: &PullRequestUrl,
        _verdict: ReviewVerdict,
        _body: &str,
    ) -> Result<(), GitHubError> {
        refused()
    }
}
