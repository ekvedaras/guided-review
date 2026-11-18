//! The issues a review is about, and where each came from. The first source that names any wins:
//! the issues GitHub links to the PR, then the ones its description names (`Part of`, closing
//! keywords GitHub did not link, `Ticket:`), then the one the guided tour names, then the one the
//! branch is named after. A rebuilt review keeps what GitHub said and reads the rest again.

use std::collections::HashMap;
use std::sync::Arc;

use crate::issue::{IssueLink, IssueUrl};
use crate::pull_request::RepoName;
use crate::review_subject::{BranchName, ReviewSubject};
use crate::tour::Tour;

use super::markdown::Markdown;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelatedIssue {
    pub link: IssueLink,
    /// Known only for an issue GitHub linked.
    pub title: Option<Arc<str>>,
}

impl RelatedIssue {
    const fn untitled(link: IssueLink) -> Self {
        Self { link, title: None }
    }

    fn of_github(url: IssueUrl) -> Self {
        Self::untitled(IssueLink::GitHub(url))
    }
}

/// Where a GitHub issue stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IssueState {
    Open,
    ClosedAsCompleted,
    ClosedAsNotPlanned,
    /// Closed for a reason GitHub names that the card does not tell apart, as a duplicate.
    Closed,
    /// A pull request named as an issue, as `Part of #12` can, once merged.
    Merged,
}

impl IssueState {
    pub fn of_github(state: &str, reason: Option<&str>) -> Self {
        match (state, reason) {
            ("OPEN", _) => Self::Open,
            ("MERGED", _) => Self::Merged,
            (_, Some("COMPLETED")) => Self::ClosedAsCompleted,
            (_, Some("NOT_PLANNED")) => Self::ClosedAsNotPlanned,
            _ => Self::Closed,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Open => "Open",
            Self::ClosedAsCompleted => "Closed as completed",
            Self::ClosedAsNotPlanned => "Closed as not planned",
            Self::Closed => "Closed",
            Self::Merged => "Merged",
        }
    }
}

/// What a GitHub issue says of itself, for the card its button shows on a hover. The body is
/// parsed here, once, so drawing the card parses nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssueDetails {
    pub title: Arc<str>,
    pub state: IssueState,
    pub author: Option<Arc<str>>,
    pub labels: Arc<[Arc<str>]>,
    pub assignees: Arc<[Arc<str>]>,
    /// `None` for an issue with no description.
    pub body: Option<Markdown>,
}

impl IssueDetails {
    pub fn markdown_of(body: &str) -> Option<Markdown> {
        (!body.trim().is_empty()).then(|| Markdown::parse(body).nested_one_level())
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
    named_in_description: Vec<RelatedIssue>,
    named_by_tour: Option<RelatedIssue>,
    of_local_branch: Option<RelatedIssue>,
    shown: Arc<[RelatedIssue]>,
    /// What GitHub last said of each shown issue; an issue no longer shown keeps its entry.
    details: HashMap<IssueUrl, Arc<IssueDetails>>,
}

impl RelatedIssues {
    pub(super) fn of(subject: &ReviewSubject, tour: &Tour, description: &str) -> Self {
        let of_local_branch = subject.local().and_then(|change_set| {
            IssueUrl::of_branch(change_set.repo()?, change_set.branch())
                .map(RelatedIssue::of_github)
        });
        let named_by_tour = tour
            .guided()
            .and_then(|guided| guided.issue.clone())
            .map(RelatedIssue::untitled);
        let mut issues = Self {
            on_github: None,
            pr_repo: subject.pull_request().map(|pr| pr.repo.clone()),
            named_in_description: Vec::new(),
            named_by_tour,
            of_local_branch,
            shown: Arc::default(),
            details: HashMap::new(),
        };
        issues.description_is(description);
        issues
    }

    pub(super) fn description_is(&mut self, description: &str) {
        self.named_in_description = self
            .pr_repo
            .as_ref()
            .map_or_else(Vec::new, |repo| IssueLink::named_in(description, repo))
            .into_iter()
            .map(RelatedIssue::untitled)
            .collect();
        self.choose_shown();
    }

    pub(super) fn carry_on_from(&mut self, replaced: Self) {
        self.on_github = replaced.on_github;
        self.details = replaced.details;
        self.choose_shown();
    }

    pub(super) fn details_say(&mut self, read: Vec<(IssueUrl, IssueDetails)>) {
        self.details.extend(
            read.into_iter()
                .map(|(url, details)| (url, Arc::new(details))),
        );
    }

    /// `None` for a ticket, which GitHub knows nothing of, and for an issue not read yet.
    pub(super) fn details_of(&self, link: &IssueLink) -> Option<Arc<IssueDetails>> {
        match link {
            IssueLink::GitHub(url) => self.details.get(url).cloned(),
            IssueLink::Ticket(_) => None,
        }
    }

    /// The shown issues GitHub has details of.
    pub(super) fn shown_on_github(&self) -> Vec<IssueUrl> {
        self.shown
            .iter()
            .filter_map(|issue| match &issue.link {
                IssueLink::GitHub(url) => Some(url.clone()),
                IssueLink::Ticket(_) => None,
            })
            .collect()
    }

    /// The issues a snapshot scene shows, with what GitHub would say of each; `unstaged` takes them off.
    #[cfg(feature = "snapshots")]
    pub(super) fn staged(&mut self, issues: Vec<(RelatedIssue, Option<IssueDetails>)>) {
        let read = issues
            .iter()
            .filter_map(|(issue, details)| match (&issue.link, details) {
                (IssueLink::GitHub(url), Some(details)) => Some((url.clone(), details.clone())),
                _ => None,
            })
            .collect();
        self.details_say(read);
        self.github_says(PullRequestIssues {
            linked: issues.into_iter().map(|(issue, _)| issue).collect(),
            head_branch: None,
        });
    }

    #[cfg(feature = "snapshots")]
    pub(super) fn unstaged(&mut self) {
        self.on_github = None;
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
            IssueUrl::of_branch(repo, branch).map(RelatedIssue::of_github)
        };
        let in_description =
            Some(self.named_in_description.clone()).filter(|issues| !issues.is_empty());
        self.shown = linked
            .or(in_description)
            .or_else(|| self.named_by_tour.clone().map(|issue| vec![issue]))
            .or_else(|| self.of_local_branch.clone().map(|issue| vec![issue]))
            .or_else(|| of_pr_branch().map(|issue| vec![issue]))
            .unwrap_or_default()
            .into();
    }
}
