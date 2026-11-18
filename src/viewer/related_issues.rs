//! The GitHub issues a review is about, and where each came from. The first source that names any
//! wins: the issues GitHub links to the PR, then the one the guided tour names, then the one the
//! branch is named after. A rebuilt review keeps what GitHub said and reads the rest again.

use std::sync::Arc;

use crate::issue::IssueUrl;
use crate::pull_request::RepoName;
use crate::review_subject::{BranchName, ReviewSubject};
use crate::tour::Tour;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelatedIssue {
    pub url: IssueUrl,
    /// Known only for an issue GitHub linked.
    pub title: Option<Arc<str>>,
}

impl RelatedIssue {
    const fn untitled(url: IssueUrl) -> Self {
        Self { url, title: None }
    }
}

/// What GitHub says of a PR's issues: the ones it would close, and the branch it is opened from.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PullRequestIssues {
    pub linked: Vec<RelatedIssue>,
    pub head_branch: Option<BranchName>,
}

#[derive(Debug, Default)]
pub(super) struct RelatedIssues {
    /// `None` until GitHub answers, and always for a local review.
    on_github: Option<PullRequestIssues>,
    /// The PR's repo, where its head branch's issue is; `None` for a local review.
    pr_repo: Option<RepoName>,
    named_by_tour: Option<RelatedIssue>,
    of_local_branch: Option<RelatedIssue>,
    shown: Arc<[RelatedIssue]>,
}

impl RelatedIssues {
    pub(super) fn of(subject: &ReviewSubject, tour: &Tour) -> Self {
        let of_local_branch = subject.local().and_then(|change_set| {
            IssueUrl::of_branch(change_set.repo()?, change_set.branch()).map(RelatedIssue::untitled)
        });
        let named_by_tour = tour
            .guided()
            .and_then(|guided| guided.issue.clone())
            .map(RelatedIssue::untitled);
        let mut issues = Self {
            on_github: None,
            pr_repo: subject.pull_request().map(|pr| pr.repo.clone()),
            named_by_tour,
            of_local_branch,
            shown: Arc::default(),
        };
        issues.choose_shown();
        issues
    }

    pub(super) fn carry_on_from(&mut self, replaced: Self) {
        self.on_github = replaced.on_github;
        self.choose_shown();
    }

    pub(super) fn github_says(&mut self, issues: PullRequestIssues) {
        self.on_github = Some(issues);
        self.choose_shown();
    }

    pub(super) fn shown(&self) -> Arc<[RelatedIssue]> {
        Arc::clone(&self.shown)
    }

    fn choose_shown(&mut self) {
        let linked = self
            .on_github
            .as_ref()
            .map(|on_github| on_github.linked.clone())
            .filter(|linked| !linked.is_empty());
        let of_pr_branch = || {
            let repo = self.pr_repo.as_ref()?;
            let branch = self.on_github.as_ref()?.head_branch.as_ref()?;
            IssueUrl::of_branch(repo, branch).map(RelatedIssue::untitled)
        };
        self.shown = linked
            .or_else(|| self.named_by_tour.clone().map(|issue| vec![issue]))
            .or_else(|| self.of_local_branch.clone().map(|issue| vec![issue]))
            .or_else(|| of_pr_branch().map(|issue| vec![issue]))
            .unwrap_or_default()
            .into();
    }
}
