use std::path::PathBuf;

use crate::github::GitHub;
use crate::pull_request::{PullRequestTips, PullRequestUrl};

/// Everything the viewer core asks of GitHub. Core actions take it as `&dyn GitHubPort`,
/// so the app can run them in the background and tests can put a fake behind it.
pub trait GitHubPort: Send + Sync {
    fn pull_request_tips(&self, pr: &PullRequestUrl) -> anyhow::Result<PullRequestTips>;

    /// Every changed file of the PR with the viewer's viewed state, all pages of it.
    /// Fails with `ViewedSyncUnavailable` when there is no token or it may not do this.
    fn viewed_files(&self, pr: &PullRequestUrl) -> anyhow::Result<Vec<ViewedFile>>;

    fn set_file_viewed(&self, pr: &PullRequestUrl, path: &str, viewed: bool) -> anyhow::Result<()>;
}

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

impl GitHubPort for GitHub {
    fn pull_request_tips(&self, pr: &PullRequestUrl) -> anyhow::Result<PullRequestTips> {
        GitHub::pull_request_tips(self, pr)
    }

    fn viewed_files(&self, pr: &PullRequestUrl) -> anyhow::Result<Vec<ViewedFile>> {
        GitHub::viewed_files(self, pr)
    }

    fn set_file_viewed(&self, pr: &PullRequestUrl, path: &str, viewed: bool) -> anyhow::Result<()> {
        GitHub::set_file_viewed(self, pr, path, viewed)
    }
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
}
