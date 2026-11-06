use std::collections::{BTreeMap, VecDeque};
use std::ops::ControlFlow;
use std::sync::Mutex;

use guided_review::pull_request::PullRequestUrl;
use guided_review::viewer::{AgentStatus, LiveTerminal, TerminalId, TourAgentPort};

/// One change the app core made through `FakeTourAgent`, in the order it made them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TourAgentCall {
    Claim {
        terminal: TerminalId,
        pr: PullRequestUrl,
    },
    Release(TerminalId),
    Show(TerminalId),
}

/// Herdr with a few terminals: which PR each is the tour agent of, and the status each reports.
pub struct FakeTourAgent {
    link: Mutex<Result<(), String>>,
    terminals: Mutex<BTreeMap<TerminalId, (Option<PullRequestUrl>, AgentStatus)>>,
    /// What `follow` reports, in order, before its stream ends.
    coming: Mutex<VecDeque<AgentStatus>>,
    calls: Mutex<Vec<TourAgentCall>>,
    live: Mutex<Vec<LiveTerminal>>,
}

impl FakeTourAgent {
    pub fn new() -> Self {
        Self {
            link: Mutex::new(Ok(())),
            terminals: Mutex::default(),
            coming: Mutex::default(),
            calls: Mutex::default(),
            live: Mutex::default(),
        }
    }

    pub fn without_herdr(self, reason: &str) -> Self {
        *self.link.lock().unwrap() = Err(reason.to_owned());
        self
    }

    pub fn with_terminal(self, terminal: &str, pr: Option<&str>, status: AgentStatus) -> Self {
        self.terminals.lock().unwrap().insert(
            TerminalId::new(terminal),
            (pr.map(|pr| pr.parse().unwrap()), status),
        );
        self
    }

    /// A live terminal running Claude in a herdr space; it can be claimed.
    pub fn claude(self, terminal: &str, space: &str) -> Self {
        self.running(terminal, true, space)
    }

    /// A live terminal running a plain shell in a herdr space; it can be claimed.
    pub fn shell(self, terminal: &str, space: &str) -> Self {
        self.running(terminal, false, space)
    }

    fn running(self, terminal: &str, runs_claude: bool, space: &str) -> Self {
        self.live.lock().unwrap().push(LiveTerminal {
            terminal: TerminalId::new(terminal),
            runs_claude,
            space: space.to_owned(),
        });
        self.with_terminal(terminal, None, AgentStatus::Unreported)
    }

    pub fn then_reports(self, statuses: &[AgentStatus]) -> Self {
        self.coming.lock().unwrap().extend(statuses.iter().copied());
        self
    }

    pub fn calls(&self) -> Vec<TourAgentCall> {
        self.calls.lock().unwrap().clone()
    }

    /// The terminals that carry the PR's label now.
    pub fn agents_of(&self, pr: &str) -> Vec<String> {
        let pr: PullRequestUrl = pr.parse().unwrap();
        self.terminals
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, (labelled, _))| labelled.as_ref() == Some(&pr))
            .map(|(terminal, _)| terminal.to_string())
            .collect()
    }

    fn record(&self, call: TourAgentCall) {
        self.calls.lock().unwrap().push(call);
    }
}

impl TourAgentPort for FakeTourAgent {
    fn link(&self) -> Result<(), String> {
        self.link.lock().unwrap().clone()
    }

    fn terminals_of(&self, pr: &PullRequestUrl) -> anyhow::Result<Vec<TerminalId>> {
        Ok(self
            .terminals
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, (labelled, _))| labelled.as_ref() == Some(pr))
            .map(|(terminal, _)| terminal.clone())
            .collect())
    }

    fn live_terminals(&self) -> anyhow::Result<Vec<LiveTerminal>> {
        Ok(self.live.lock().unwrap().clone())
    }

    fn claim(&self, terminal: &TerminalId, pr: &PullRequestUrl) -> anyhow::Result<()> {
        self.record(TourAgentCall::Claim {
            terminal: terminal.clone(),
            pr: pr.clone(),
        });
        let mut terminals = self.terminals.lock().unwrap();
        let (labelled, _) = terminals
            .get_mut(terminal)
            .ok_or_else(|| anyhow::anyhow!("no terminal {terminal}"))?;
        *labelled = Some(pr.clone());
        Ok(())
    }

    fn release(&self, terminal: &TerminalId) -> anyhow::Result<()> {
        self.record(TourAgentCall::Release(terminal.clone()));
        if let Some((labelled, _)) = self.terminals.lock().unwrap().get_mut(terminal) {
            *labelled = None;
        }
        Ok(())
    }

    fn status(&self, terminal: &TerminalId) -> anyhow::Result<AgentStatus> {
        self.terminals
            .lock()
            .unwrap()
            .get(terminal)
            .map(|(_, status)| *status)
            .ok_or_else(|| anyhow::anyhow!("no terminal {terminal}"))
    }

    fn follow(
        &self,
        _terminal: &TerminalId,
        on_status: &mut dyn FnMut(AgentStatus) -> ControlFlow<()>,
    ) -> anyhow::Result<()> {
        while let Some(status) = self.coming.lock().unwrap().pop_front() {
            if on_status(status).is_break() {
                break;
            }
        }
        Ok(())
    }

    fn show(&self, terminal: &TerminalId) -> anyhow::Result<()> {
        self.record(TourAgentCall::Show(terminal.clone()));
        Ok(())
    }
}
