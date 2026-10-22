use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use guided_review::pull_request::{PullRequestTips, PullRequestUrl, Sha};
use guided_review::viewer::{AgentLaunch, AgentLauncher, AgentPort, Ask, AskStore, GitHubPort};

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

/// Replays scripted answers, one stream line at a time, one scripted turn per question.
pub struct FakeAgent {
    pub sent: SentLog,
    turns: VecDeque<Vec<String>>,
}

impl FakeAgent {
    pub fn answering(turns: &[&str], sent: &SentLog) -> Self {
        Self {
            sent: sent.clone(),
            turns: turns
                .iter()
                .map(|turn| turn.lines().map(str::to_string).collect())
                .collect(),
        }
    }
}

impl AgentPort for FakeAgent {
    fn ask(&mut self, message: &str, on_line: &mut dyn FnMut(&str)) -> anyhow::Result<()> {
        self.sent.push(format!("agent: {message}"));
        let turn = self.turns.pop_front().unwrap_or_default();
        turn.iter().for_each(|line| on_line(line));
        Ok(())
    }
}

/// Hands out one `FakeAgent` per start and logs how it was launched.
pub struct FakeAgentLauncher {
    pub sent: SentLog,
    pub turns: Vec<String>,
}

impl AgentLauncher for FakeAgentLauncher {
    fn start(&self, launch: &AgentLaunch) -> anyhow::Result<Box<dyn AgentPort>> {
        self.sent.push(format!(
            "agent started in {} with {} / {}{}",
            launch.cwd.display(),
            launch.model,
            launch.effort,
            launch
                .resume
                .as_ref()
                .map(|id| format!(", resuming {id}"))
                .unwrap_or_default()
        ));
        let turns: Vec<&str> = self.turns.iter().map(String::as_str).collect();
        Ok(Box::new(FakeAgent::answering(&turns, &self.sent)))
    }
}

/// Stands in for the per-PR state file.
#[derive(Clone, Default)]
pub struct FakeAskStore(Arc<Mutex<Vec<Ask>>>);

impl FakeAskStore {
    pub fn saved(&self) -> Vec<Ask> {
        self.0.lock().unwrap().clone()
    }
}

impl AskStore for FakeAskStore {
    fn load(&self) -> anyhow::Result<Vec<Ask>> {
        Ok(self.saved())
    }

    fn save(&self, asks: &[Ask]) -> anyhow::Result<()> {
        *self.0.lock().unwrap() = asks.to_vec();
        Ok(())
    }
}
