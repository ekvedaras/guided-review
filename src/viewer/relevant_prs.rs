use super::ports::GitHubError;
use crate::pull_request::PullRequestUrl;

/// The open PRs the reviewer has something to do with, for the PR switcher.
pub trait RelevantPullRequestsPort: Send + Sync {
    /// Every open PR that involves the viewer or asks one of their teams for a review, newest
    /// update first, one entry per PR url.
    fn relevant_pull_requests(&self) -> Result<RelevantPullRequests, GitHubError>;
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RelevantPullRequests {
    pub pull_requests: Vec<RelevantPullRequest>,
    /// Teams whose review requests were not searched, because only so many searches are made.
    pub teams_left_out: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelevantPullRequest {
    pub url: PullRequestUrl,
    pub title: String,
    pub author: String,
    pub is_draft: bool,
    /// As GitHub writes it, `2026-11-05T13:35:33Z`, which sorts as time does.
    pub updated_at: String,
    pub why_listed: WhyListed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WhyListed {
    /// The viewer wrote it.
    Mine,
    /// The viewer or one of their teams is asked to review it.
    ReviewRequested,
    /// The viewer is in it some other way: reviewed, commented, assigned or mentioned.
    Involved,
}
