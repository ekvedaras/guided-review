use std::sync::{Arc, Mutex};

use guided_review::pull_request::{PullRequestTips, PullRequestUrl, Sha};
use guided_review::viewer::{AgentPort, GitHubPort};

/// What the app core sent out, in order, across both fakes.
#[derive(Clone, Default)]
pub struct SentLog(Arc<Mutex<Vec<String>>>);

impl SentLog {
    pub fn entries(&self) -> Vec<String> {
        self.0.lock().unwrap().clone()
    }

    fn push(&self, entry: String) {
        self.0.lock().unwrap().push(entry);
    }
}

pub struct FakeGitHub {
    pub sent: SentLog,
    pub tips: (Sha, Sha),
}

impl FakeGitHub {
    pub fn with_pull(base_branch_tip: &str, head: &str, sent: &SentLog) -> Self {
        Self {
            sent: sent.clone(),
            tips: (Sha::new(base_branch_tip), Sha::new(head)),
        }
    }
}

impl GitHubPort for FakeGitHub {
    fn pull_request_tips(&self, pr: &PullRequestUrl) -> anyhow::Result<PullRequestTips> {
        self.sent.push(format!("github: tips of {pr}"));
        Ok(PullRequestTips {
            base_branch_tip: self.tips.0.clone(),
            head: self.tips.1.clone(),
        })
    }
}

/// Replays a scripted answer, one stream line at a time.
pub struct FakeAgent {
    pub sent: SentLog,
    pub answer: Vec<String>,
}

impl FakeAgent {
    pub fn answering(lines: &[&str], sent: &SentLog) -> Self {
        Self {
            sent: sent.clone(),
            answer: lines.iter().map(|line| line.to_string()).collect(),
        }
    }
}

impl AgentPort for FakeAgent {
    fn ask(&mut self, question: &str, on_line: &mut dyn FnMut(&str)) -> anyhow::Result<()> {
        self.sent.push(format!("agent: {question}"));
        self.answer.iter().for_each(|line| on_line(line));
        Ok(())
    }
}
