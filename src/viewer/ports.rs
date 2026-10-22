use std::path::PathBuf;
use std::sync::Arc;

use super::comments::{CommentPlace, SendMode};
use super::finish_review::ReviewVerdict;
use super::reactions::Reaction;
use super::threads::ReviewThread;
use crate::pull_request::{PullRequestTips, PullRequestUrl};

pub trait PullRequestTipsPort: Send + Sync {
    fn pull_request_tips(&self, pr: &PullRequestUrl) -> anyhow::Result<PullRequestTips>;
}

pub trait ViewedFilesPort: Send + Sync {
    /// Every changed file of the PR with the viewer's viewed state, all pages of it.
    /// Fails with `ViewedSyncUnavailable` when there is no token or it may not do this.
    fn viewed_files(&self, pr: &PullRequestUrl) -> anyhow::Result<Vec<ViewedFile>>;

    fn set_file_viewed(&self, pr: &PullRequestUrl, path: &str, viewed: bool) -> anyhow::Result<()>;
}

/// Reading review threads and acting on them: replies, resolving, reactions.
pub trait ReviewThreadsPort: Send + Sync {
    fn review_threads(&self, pr: &PullRequestUrl) -> anyhow::Result<Vec<ReviewThread>>;

    /// Adds a reply to the pending review, or with `CommentNow` posts it at once.
    fn reply_to_thread(
        &self,
        pr: &PullRequestUrl,
        thread_id: &str,
        body: &str,
        mode: SendMode,
    ) -> anyhow::Result<()>;

    fn set_thread_resolved(&self, thread_id: &str, resolved: bool) -> anyhow::Result<()>;

    fn set_reaction(&self, comment_id: &str, reaction: Reaction, on: bool) -> anyhow::Result<()>;
}

/// The viewer's own review: what it holds so far, new comments, and sending it.
pub trait ReviewWritesPort: Send + Sync {
    /// Whether the viewer has a pending review on the PR, one made here or on the web.
    fn has_pending_review(&self, pr: &PullRequestUrl) -> anyhow::Result<bool>;

    /// Whether the viewer wrote the PR. GitHub refuses their approval or change request.
    fn viewer_authored(&self, pr: &PullRequestUrl) -> anyhow::Result<bool>;

    /// Adds a thread to the pending review, or starts one. Fails with `ThreadNotCreated`
    /// when GitHub answers without a thread.
    fn add_review_thread(
        &self,
        pr: &PullRequestUrl,
        place: &CommentPlace,
        body: &str,
    ) -> anyhow::Result<()>;

    /// Posts one comment at once as a review of its own. Fails with `PendingReviewExists`
    /// when the viewer has a pending review, which GitHub would not post past.
    fn comment_now(
        &self,
        pr: &PullRequestUrl,
        place: &CommentPlace,
        body: &str,
    ) -> anyhow::Result<()>;

    /// Sends the pending review, or a new review when there is none.
    fn submit_review(
        &self,
        pr: &PullRequestUrl,
        verdict: ReviewVerdict,
        body: &str,
    ) -> anyhow::Result<()>;
}

/// Everything the viewer core asks of GitHub, for the app that holds one adapter for all of it.
/// Core actions take only the port they need, so tests can put a fake behind it.
pub trait GitHubPort:
    PullRequestTipsPort + ViewedFilesPort + ReviewThreadsPort + ReviewWritesPort
{
}

impl<T> GitHubPort for T where
    T: PullRequestTipsPort + ViewedFilesPort + ReviewThreadsPort + ReviewWritesPort
{
}

/// GitHub answered a new thread with `thread: null` and no error. It leaves an empty pending review behind.
#[derive(Debug)]
pub struct ThreadNotCreated(pub String);

impl std::fmt::Display for ThreadNotCreated {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ThreadNotCreated {}

/// "User can only have one pending review per pull request."
#[derive(Debug)]
pub struct PendingReviewExists;

impl std::fmt::Display for PendingReviewExists {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("there is a pending review already")
    }
}

impl std::error::Error for PendingReviewExists {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewedState {
    Viewed,
    Unviewed,
    /// Viewed once, then changed by a new commit. Counts as not viewed.
    Dismissed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViewedFile {
    pub path: String,
    pub state: ViewedState,
}

/// No token, or GitHub refused it. The app keeps working with viewed sync switched off.
#[derive(Debug)]
pub struct ViewedSyncUnavailable(pub String);

impl std::fmt::Display for ViewedSyncUnavailable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ViewedSyncUnavailable {}

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
    fn refresh(&self, pr: &PullRequestUrl) -> anyhow::Result<()>;
}
