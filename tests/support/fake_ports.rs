use std::collections::{BTreeMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use guided_review::pull_request::{PullRequestTips, PullRequestUrl, Sha};
use guided_review::viewer::{
    AgentLaunch, AgentLauncher, AgentPort, Ask, AskStore, GitHubPort, ReviewThread, ViewedFile,
    ViewedState, ViewedSyncUnavailable,
};

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
    viewed: Mutex<BTreeMap<String, ViewedState>>,
    pushes_fail: AtomicBool,
    refuses_with: Mutex<Option<String>>,
    pub threads: Vec<ReviewThread>,
}

impl FakeGitHub {
    pub fn with_pull(base_branch_tip: &str, head: &str, sent: &SentLog) -> Self {
        Self {
            sent: sent.clone(),
            tips: (Sha::new(base_branch_tip), Sha::new(head)),
            viewed: Mutex::default(),
            pushes_fail: AtomicBool::new(false),
            refuses_with: Mutex::default(),
            threads: Vec::new(),
        }
    }

    pub fn viewing(self, path: &str, state: ViewedState) -> Self {
        self.set_viewed(path, state);
        self
    }

    /// A change made on the GitHub web page.
    pub fn set_viewed(&self, path: &str, state: ViewedState) {
        self.viewed.lock().unwrap().insert(path.to_string(), state);
    }

    pub fn viewed_state(&self, path: &str) -> ViewedState {
        self.viewed.lock().unwrap()[path]
    }

    pub fn pushes_fail(&self, fail: bool) {
        self.pushes_fail.store(fail, Ordering::SeqCst);
    }

    pub fn refuse_everything(&self, reason: &str) {
        *self.refuses_with.lock().unwrap() = Some(reason.to_string());
    }

    fn check_access(&self) -> anyhow::Result<()> {
        match self.refuses_with.lock().unwrap().clone() {
            Some(reason) => Err(ViewedSyncUnavailable(reason).into()),
            None => Ok(()),
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

    fn viewed_files(&self, pr: &PullRequestUrl) -> anyhow::Result<Vec<ViewedFile>> {
        self.sent.push(format!("github: viewed files of {pr}"));
        self.check_access()?;
        Ok(self
            .viewed
            .lock()
            .unwrap()
            .iter()
            .map(|(path, state)| ViewedFile {
                path: path.clone(),
                state: *state,
            })
            .collect())
    }

    fn set_file_viewed(&self, pr: &PullRequestUrl, path: &str, viewed: bool) -> anyhow::Result<()> {
        self.sent
            .push(format!("github: viewed={viewed} {path} of {pr}"));
        self.check_access()?;
        if self.pushes_fail.load(Ordering::SeqCst) {
            anyhow::bail!("network is down");
        }
        let state = if viewed {
            ViewedState::Viewed
        } else {
            ViewedState::Unviewed
        };
        self.set_viewed(path, state);
        Ok(())
    }

    fn review_threads(&self, pr: &PullRequestUrl) -> anyhow::Result<Vec<ReviewThread>> {
        self.sent.push(format!("github: threads of {pr}"));
        Ok(self.threads.clone())
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
