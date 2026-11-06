use std::collections::{BTreeMap, VecDeque};
use std::ops::ControlFlow;
use std::sync::Mutex;

use guided_review::pull_request::PullRequestUrl;
use std::path::PathBuf;

use guided_review::viewer::{
    AgentStatus, AppAgent, LiveTerminal, SendOutcome, TerminalId, TourAgentPort, TourAgentStart,
};

/// One change the app core made through `FakeTourAgent`, in the order it made them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TourAgentCall {
    Claim {
        terminal: TerminalId,
        pr: PullRequestUrl,
    },
    Release(TerminalId),
    Show(TerminalId),
    Start {
        name: String,
        pr: PullRequestUrl,
        folder: PathBuf,
        first_task: String,
    },
    Kill(TerminalId),
    /// Text typed into the terminal.
    Send {
        terminal: TerminalId,
        text: String,
    },
}

/// Herdr with a few terminals: which PR each is the tour agent of, and the status each reports.
pub struct FakeTourAgent {
    link: Mutex<Result<(), String>>,
    terminals: Mutex<BTreeMap<TerminalId, (Option<PullRequestUrl>, AgentStatus)>>,
    /// What `follow` reports, in order, before its stream ends.
    coming: Mutex<VecDeque<AgentStatus>>,
    calls: Mutex<Vec<TourAgentCall>>,
    live: Mutex<Vec<LiveTerminal>>,
    /// The terminals the app started, with when their status last changed.
    started_by_app: Mutex<BTreeMap<TerminalId, u64>>,
    /// What every terminal shows, top line first.
    screen: Mutex<Vec<String>>,
}

impl FakeTourAgent {
    pub fn new() -> Self {
        Self {
            link: Mutex::new(Ok(())),
            terminals: Mutex::default(),
            coming: Mutex::default(),
            calls: Mutex::default(),
            live: Mutex::default(),
            started_by_app: Mutex::default(),
            screen: Mutex::default(),
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

    /// A tour agent the app started, last changed `since_ms` after the epoch.
    pub fn app_started(self, terminal: &str, pr: &str, status: AgentStatus, since_ms: u64) -> Self {
        self.started_by_app
            .lock()
            .unwrap()
            .insert(TerminalId::new(terminal), since_ms);
        self.with_terminal(terminal, Some(pr), status)
    }

    pub fn set_status(&self, terminal: &str, status: AgentStatus) {
        if let Some((_, current)) = self
            .terminals
            .lock()
            .unwrap()
            .get_mut(&TerminalId::new(terminal))
        {
            *current = status;
        }
    }

    pub fn live_terminal_addresses(&self) -> Vec<String> {
        self.terminals
            .lock()
            .unwrap()
            .keys()
            .map(ToString::to_string)
            .collect()
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

    fn send_if_idle(&self, terminal: &TerminalId, text: &str) -> anyhow::Result<SendOutcome> {
        let mut terminals = self.terminals.lock().unwrap();
        let (_, status) = terminals
            .get_mut(terminal)
            .ok_or_else(|| anyhow::anyhow!("no terminal {terminal}"))?;
        if *status != AgentStatus::Idle {
            return Ok(SendOutcome::Refused(format!(
                "{terminal} is {status}, not idle"
            )));
        }
        *status = AgentStatus::Working;
        drop(terminals);
        self.record(TourAgentCall::Send {
            terminal: terminal.clone(),
            text: text.to_owned(),
        });
        Ok(SendOutcome::Delivered)
    }

    fn show(&self, terminal: &TerminalId) -> anyhow::Result<()> {
        self.record(TourAgentCall::Show(terminal.clone()));
        Ok(())
    }

    fn app_started(&self) -> anyhow::Result<Vec<AppAgent>> {
        let terminals = self.terminals.lock().unwrap();
        Ok(self
            .started_by_app
            .lock()
            .unwrap()
            .iter()
            .filter_map(|(terminal, since)| {
                let (pr, status) = terminals.get(terminal)?;
                Some(AppAgent {
                    terminal: terminal.clone(),
                    pr: pr.clone(),
                    status: *status,
                    status_since_ms: *since,
                })
            })
            .collect())
    }

    fn start(&self, start: &TourAgentStart) -> anyhow::Result<TerminalId> {
        self.record(TourAgentCall::Start {
            name: start.name.clone(),
            pr: start.pr.clone(),
            folder: start.folder.clone(),
            first_task: start.first_task.clone(),
        });
        let mut terminals = self.terminals.lock().unwrap();
        let terminal = TerminalId::new(format!("t{}", 100 + terminals.len()));
        terminals.insert(
            terminal.clone(),
            (Some(start.pr.clone()), AgentStatus::Working),
        );
        self.started_by_app
            .lock()
            .unwrap()
            .insert(terminal.clone(), u64::MAX);
        Ok(terminal)
    }

    fn kill(&self, terminal: &TerminalId) -> anyhow::Result<()> {
        self.record(TourAgentCall::Kill(terminal.clone()));
        self.terminals.lock().unwrap().remove(terminal);
        self.started_by_app.lock().unwrap().remove(terminal);
        Ok(())
    }

    fn screen_lines(&self, _terminal: &TerminalId, count: usize) -> anyhow::Result<Vec<String>> {
        let screen = self.screen.lock().unwrap();
        Ok(screen
            .iter()
            .skip(screen.len().saturating_sub(count))
            .cloned()
            .collect())
    }
}
