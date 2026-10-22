use crate::github::GitHub;
use crate::pull_request::{PullRequestTips, PullRequestUrl};

/// Everything the viewer core asks of GitHub. Core actions take it as `&dyn GitHubPort`,
/// so the app can run them in the background and tests can put a fake behind it.
pub trait GitHubPort: Send + Sync {
    fn pull_request_tips(&self, pr: &PullRequestUrl) -> anyhow::Result<PullRequestTips>;
}

impl GitHubPort for GitHub {
    fn pull_request_tips(&self, pr: &PullRequestUrl) -> anyhow::Result<PullRequestTips> {
        GitHub::pull_request_tips(self, pr)
    }
}

/// The local AI behind Ask: start a conversation, send a message, stream the answer.
pub trait AgentPort: Send {
    fn ask(&mut self, question: &str, on_line: &mut dyn FnMut(&str)) -> anyhow::Result<()>;
}
