use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::comments::{CommentPlace, SendMode};
use super::host_outage::OutageCause;
use super::merging::MergeChange;
use super::pr_origin::PullRequestOrigin;
use super::pr_reviewers::PullRequestReviews;
use super::pr_status::{PrStatus, Readiness};
use super::reactions::{Reaction, ReactionChange};
use super::related_issues::{IssueDetails, PullRequestIssues};
use super::review_thread::{CommentId, ReviewThread, ThreadId};
use crate::file_path::FilePath;
use crate::issue::IssueUrl;
use crate::pull_request::{ChangeTips, PullRequestUrl, RepoName};
use crate::review_subject::{BranchName, ReviewSubject};

pub trait ChangeTipsPort: Send + Sync {
    fn pull_request_tips(&self, pr: &PullRequestUrl) -> Result<ChangeTips, HostError>;
}

/// An open PR whose head is a branch of the repo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PullOfBranch {
    pub pr: PullRequestUrl,
    pub title: String,
}

pub trait PullsOfBranchPort: Send + Sync {
    /// The open PR whose head is `branch` of `repo` itself, when there is one.
    fn open_pull_of_branch(
        &self,
        repo: &RepoName,
        branch: &BranchName,
    ) -> Result<Option<PullOfBranch>, HostError>;

    /// The branch of the PR's own repo that the PR is opened from; `None` for a PR from a fork.
    fn pull_request_head_branch(
        &self,
        pr: &PullRequestUrl,
    ) -> Result<Option<BranchName>, HostError>;
}

pub trait ViewedFilesPort: Send + Sync {
    /// Every changed file of the PR with the viewer's viewed state, all pages of it.
    /// Fails with `NoAccess` when there is no token or it may not do this.
    fn viewed_files(&self, pr: &PullRequestUrl) -> Result<Vec<ViewedFile>, HostError>;

    fn set_file_viewed(
        &self,
        pr: &PullRequestUrl,
        path: &FilePath,
        viewed: Viewed,
    ) -> Result<(), HostError>;
}

pub trait PullRequestDescriptionPort: Send + Sync {
    /// The PR's body as markdown; empty when it has none.
    fn pull_request_description(&self, pr: &PullRequestUrl) -> Result<String, HostError>;

    fn set_pull_request_description(&self, pr: &PullRequestUrl, body: &str) -> anyhow::Result<()>;

    /// The issues the PR would close, as its description or GitHub's sidebar links them.
    fn pull_request_issues(&self, pr: &PullRequestUrl) -> Result<PullRequestIssues, HostError>;
}

/// Who opened a PR and the branches it merges from and into, which the title bar shows.
pub trait PullRequestOriginPort: Send + Sync {
    fn pull_request_origin(&self, pr: &PullRequestUrl) -> Result<PullRequestOrigin, HostError>;
}

/// What GitHub says of issues, which a review shows on a hover of its issue button.
pub trait IssueDetailsPort: Send + Sync {
    /// The details of each of `issues` in one read, in no set order.
    fn issue_details(
        &self,
        issues: &[IssueUrl],
    ) -> Result<Vec<(IssueUrl, IssueDetails)>, HostError>;
}

/// The viewer's reactions on the PR itself, which GitHub shows under its description.
/// The PR's status: whether it is a draft, its checks, and what the viewer may change.
pub trait PullRequestStatusPort: Send + Sync {
    fn pull_request_status(&self, pr: &PullRequestUrl) -> Result<PrStatus, HostError>;

    /// Converts the PR to a draft or marks it ready for review.
    fn set_pull_request_readiness(
        &self,
        pr: &PullRequestUrl,
        readiness: Readiness,
    ) -> Result<(), HostError>;

    /// Turns auto-merge on or off, or merges the PR now.
    fn change_merging(&self, pr: &PullRequestUrl, change: MergeChange) -> Result<(), HostError>;
}

pub trait PullRequestReactionsPort: Send + Sync {
    fn set_pull_request_reaction(
        &self,
        pr: &PullRequestUrl,
        reaction: Reaction,
        change: ReactionChange,
    ) -> Result<(), HostError>;
}

pub trait PictureUploadPort: Send + Sync {
    /// The address GitHub keeps the picture at, open only to those who can see the PR's repository.
    fn upload_picture(&self, pr: &PullRequestUrl, picture: &Path) -> anyhow::Result<String>;
}

/// Reading review threads and acting on them: replies, resolving, reactions.
pub trait ReviewThreadsPort: Send + Sync {
    fn review_threads(&self, pr: &PullRequestUrl) -> Result<Vec<ReviewThread>, HostError>;

    /// Adds a reply to the pending review, or with `CommentNow` posts it at once. Returns the new comment.
    fn reply_to_thread(
        &self,
        pr: &PullRequestUrl,
        thread_id: &ThreadId,
        body: &str,
        mode: SendMode,
    ) -> Result<CommentId, HostError>;

    fn set_thread_resolved(
        &self,
        thread_id: &ThreadId,
        resolution: Resolution,
    ) -> Result<(), HostError>;

    fn set_reaction(
        &self,
        comment_id: &CommentId,
        reaction: Reaction,
        change: ReactionChange,
    ) -> Result<(), HostError>;

    /// Replaces the body of one of the viewer's own comments, sent or still in the pending review.
    fn edit_comment(&self, comment_id: &CommentId, body: &str) -> Result<(), HostError>;

    /// Removes one of the viewer's own comments, sent or still in the pending review.
    fn delete_comment(&self, comment_id: &CommentId) -> Result<(), HostError>;
}

/// The viewer's own review: what it holds so far, new comments, and sending it.
pub trait ReviewWritesPort: Send + Sync {
    /// Whether the viewer has a pending review on the PR, one made here or on the web.
    fn has_pending_review(&self, pr: &PullRequestUrl) -> Result<bool, HostError>;

    /// The PR's reviews: the latest one the viewer sent, from here or from the web (a pending
    /// review is not sent, so it is not this), and where each reviewer stands.
    fn reviews(&self, pr: &PullRequestUrl) -> Result<PullRequestReviews, HostError>;

    /// Whether the viewer wrote the PR. GitHub refuses their approval or change request.
    fn viewer_authored(&self, pr: &PullRequestUrl) -> Result<bool, HostError>;

    /// Adds a thread to the pending review, or starts one. Fails with `ThreadNotCreated`
    /// when GitHub answers without a thread.
    fn add_review_thread(
        &self,
        pr: &PullRequestUrl,
        place: &CommentPlace,
        body: &str,
    ) -> Result<(), HostError>;

    /// Posts one comment at once as a review of its own. Fails with `PendingReviewExists`
    /// when the viewer has a pending review, which GitHub would not post past.
    fn comment_now(
        &self,
        pr: &PullRequestUrl,
        place: &CommentPlace,
        body: &str,
    ) -> Result<(), HostError>;

    /// Sends the pending review, or a new review when there is none.
    fn submit_review(
        &self,
        pr: &PullRequestUrl,
        verdict: ReviewVerdict,
        body: &str,
    ) -> Result<(), HostError>;
}

/// Everything the viewer core asks of the code host (GitHub now), for the app that holds one
/// adapter for all of it.
/// Core actions take only the port they need, so tests can put a fake behind it.
pub trait CodeHost:
    ChangeTipsPort
    + PullsOfBranchPort
    + PullRequestDescriptionPort
    + IssueDetailsPort
    + PullRequestOriginPort
    + PullRequestReactionsPort
    + PullRequestStatusPort
    + PictureUploadPort
    + ViewedFilesPort
    + ReviewThreadsPort
    + ReviewWritesPort
{
}

impl<T> CodeHost for T where
    T: ChangeTipsPort
        + PullsOfBranchPort
        + PullRequestDescriptionPort
        + IssueDetailsPort
        + PullRequestOriginPort
        + PullRequestReactionsPort
        + PullRequestStatusPort
        + PictureUploadPort
        + ViewedFilesPort
        + ReviewThreadsPort
        + ReviewWritesPort
{
}

/// A code host failure a caller tells apart from the rest.
#[derive(Debug, thiserror::Error)]
pub enum HostError {
    /// No token, or GitHub refused it.
    #[error("{0}")]
    NoAccess(NoAccess),
    /// "User can only have one pending review per pull request."
    #[error("there is a pending review already")]
    PendingReviewExists,
    /// GitHub answered a new thread with `thread: null` and no error. It leaves an empty pending review behind.
    #[error("{0}")]
    ThreadNotCreated(String),
    /// A server error, a timeout or no connection: GitHub as a whole is not answering.
    #[error("GitHub is not answering ({0})")]
    NotAnswering(OutageCause),
    /// Built from an `anyhow::Error` by `From`, which tells an outage from the rest.
    #[error(transparent)]
    Other(anyhow::Error),
}

impl HostError {
    /// What went wrong without what was being done: the sidebar says that itself.
    pub fn short_reason(&self) -> String {
        match self {
            Self::NoAccess(no_access) => no_access.reason().to_owned(),
            Self::Other(error) => error.root_cause().to_string(),
            typed => typed.to_string(),
        }
    }

    /// Says what was being done. `PendingReviewExists` and `ThreadNotCreated` already say it.
    #[must_use]
    pub fn context(self, context: impl std::fmt::Display + Send + Sync + 'static) -> Self {
        match self {
            Self::Other(error) => Self::Other(error.context(context)),
            Self::NoAccess(no_access) => Self::NoAccess(no_access.while_doing(context)),
            typed => typed,
        }
    }
}

/// Why GitHub cannot be used, and what failed because of it. A refusal names only the reason;
/// a failure shown as such says what was being done first.
#[derive(Debug)]
pub struct NoAccess {
    reason: String,
    doing: Option<String>,
}

impl NoAccess {
    pub fn new(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
            doing: None,
        }
    }

    pub fn reason(&self) -> &str {
        &self.reason
    }

    pub fn into_reason(self) -> String {
        self.reason
    }

    fn while_doing(self, doing: impl std::fmt::Display) -> Self {
        let doing = match self.doing {
            Some(inner) => format!("{doing}: {inner}"),
            None => doing.to_string(),
        };
        Self {
            doing: Some(doing),
            ..self
        }
    }
}

impl std::fmt::Display for NoAccess {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.doing {
            Some(doing) => write!(f, "{doing}: {}", self.reason),
            None => f.write_str(&self.reason),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewedState {
    Viewed,
    Unviewed,
    /// Viewed once, then changed by a new commit. Counts as not viewed.
    Dismissed,
}

impl ViewedState {
    pub fn viewed(self) -> Viewed {
        match self {
            Self::Viewed => Viewed::Yes,
            Self::Unviewed | Self::Dismissed => Viewed::No,
        }
    }
}

/// Whether the viewer wants a file marked viewed. Saved as a plain true or false, as before.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "bool", into = "bool")]
pub enum Viewed {
    Yes,
    No,
}

impl From<bool> for Viewed {
    fn from(viewed: bool) -> Self {
        if viewed { Self::Yes } else { Self::No }
    }
}

impl From<Viewed> for bool {
    fn from(viewed: Viewed) -> Self {
        viewed == Viewed::Yes
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resolution {
    Resolved,
    Unresolved,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViewedFile {
    pub path: FilePath,
    pub state: ViewedState,
}

/// How to start the local AI for one Ask.
#[derive(Debug, Clone)]
pub struct AgentLaunch {
    /// The repo checkout the AI reads code in.
    pub cwd: PathBuf,
    pub model: String,
    pub effort: String,
    pub system_prompt: String,
    /// The earlier session of an Ask that survived an app restart.
    pub resume: Option<String>,
}

/// Starts a live conversation. Starting must not wait for the first answer.
pub trait AgentLauncher: Send + Sync {
    fn start(&self, launch: &AgentLaunch) -> anyhow::Result<Box<dyn AgentPort>>;
}

/// One live conversation with the local AI. The core reads its raw stream lines
/// and turns them into `AgentEvent`s, so adapters stay thin.
pub trait AgentPort: Send {
    /// Sends one message and blocks until the answer to it is complete,
    /// passing every stream line to `on_line` as it arrives.
    fn ask(&mut self, message: &str, on_line: &mut dyn FnMut(&str)) -> anyhow::Result<()>;

    /// Stops the turn `ask` is waiting on, from another thread. The turn then ends as usual
    /// and the conversation stays live for the next question.
    fn turn_stopper(&self) -> Arc<dyn TurnStopper>;
}

pub trait TurnStopper: Send + Sync {
    fn stop_turn(&self) -> anyhow::Result<()>;
}

/// Starts the local AI on the tour skill to Refresh the tour of a PR. Blocks until it is done.
pub trait RefreshWriter: Send + Sync {
    /// Passes every stream line of the AI to `on_line` as it arrives. The core reads them, so adapters stay
    /// thin. An `Ok` says only that the AI ran to its end, not that it refreshed the tour.
    fn refresh(&self, subject: &ReviewSubject, on_line: &mut dyn FnMut(&str))
    -> anyhow::Result<()>;
}

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
