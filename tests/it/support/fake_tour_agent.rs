use std::collections::{BTreeMap, VecDeque};
use std::ops::ControlFlow;
use std::sync::Mutex;

use guided_review::claude_cli::{review_agent_denied_tools, review_agent_tools};
use guided_review::review_subject::ReviewSubject;
use std::path::PathBuf;

use guided_review::viewer::{
    AgentLinkOff, AgentRole, AgentStatus, AppAgent, HerdrRefused, LiveTerminal, ReviewAgentStart,
    SendOutcome, TerminalId, TourAgentPort, TourAgentStart,
};

/// One change the app core made through `FakeTourAgent`, in the order it made them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TourAgentCall {
    Claim {
        terminal: TerminalId,
        subject: ReviewSubject,
    },
    Release(TerminalId),
    ClaimReview {
        terminal: TerminalId,
        subject: ReviewSubject,
    },
    ReleaseReview(TerminalId),
    Show(TerminalId),
    Start {
        name: String,
        subject: ReviewSubject,
        folder: PathBuf,
        first_task: String,
    },
    StartReview {
        name: String,
        subject: ReviewSubject,
        folder: PathBuf,
        first_task: String,
        tools: String,
        denied_tools: String,
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
    link: Mutex<Result<(), AgentLinkOff>>,
    /// Herdr answers `server status` but refuses to list terminals, with this reason.
    refusing: Mutex<Option<String>>,
    terminals: Mutex<BTreeMap<TerminalId, (Option<ReviewSubject>, AgentStatus)>>,
    /// The terminals labelled as a review agent, which is a label of its own beside the tour agent's.
    reviewing: Mutex<BTreeMap<TerminalId, ReviewSubject>>,
    /// Herdr fails to start an agent, with this reason.
    start_refusal: Mutex<Option<String>>,
    /// What `status` reports after a send, one per look, the last one from then on.
    after_send: Mutex<Vec<AgentStatus>>,
    /// What the looks at the status after the send report now.
    looks: Mutex<VecDeque<AgentStatus>>,
    /// What `follow` reports, in order, before its stream ends.
    coming: Mutex<VecDeque<AgentStatus>>,
    calls: Mutex<Vec<TourAgentCall>>,
    live: Mutex<Vec<LiveTerminal>>,
    /// The terminals the app started, with when their status last changed.
    started_by_app: Mutex<BTreeMap<TerminalId, u64>>,
    /// What every terminal shows, top line first.
    screen: Mutex<Vec<String>>,
    /// Terminals whose screen has not settled since they started, so herdr types nothing into them.
    starting: Mutex<Vec<TerminalId>>,
}

impl FakeTourAgent {
    pub fn new() -> Self {
        Self {
            link: Mutex::new(Ok(())),
            refusing: Mutex::new(None),
            terminals: Mutex::default(),
            reviewing: Mutex::default(),
            start_refusal: Mutex::new(None),
            after_send: Mutex::default(),
            looks: Mutex::default(),
            coming: Mutex::default(),
            calls: Mutex::default(),
            live: Mutex::default(),
            started_by_app: Mutex::default(),
            screen: Mutex::default(),
            starting: Mutex::default(),
        }
    }

    pub fn without_herdr(self, off: AgentLinkOff) -> Self {
        *self.link.lock().unwrap() = Err(off);
        self
    }

    /// Herdr is up but fails every command with this reason.
    pub fn refusing(self, reason: &str) -> Self {
        *self.refusing.lock().unwrap() = Some(reason.to_owned());
        self
    }

    /// Herdr works again, however it failed.
    pub fn recovers(&self) {
        *self.link.lock().unwrap() = Ok(());
        *self.refusing.lock().unwrap() = None;
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

    /// A terminal the reviewer picked as the subject's review agent: labelled, not started by the app.
    pub fn labelled_review(self, terminal: &str, subject: &str) -> Self {
        self.reviewing
            .lock()
            .unwrap()
            .insert(TerminalId::new(terminal), subject.parse().unwrap());
        self
    }

    /// A review agent the app started, last changed `since_ms` after the epoch.
    pub fn review_agent(
        self,
        terminal: &str,
        subject: &str,
        status: AgentStatus,
        since_ms: u64,
    ) -> Self {
        self.reviewing
            .lock()
            .unwrap()
            .insert(TerminalId::new(terminal), subject.parse().unwrap());
        self.started_by_app
            .lock()
            .unwrap()
            .insert(TerminalId::new(terminal), since_ms);
        self.with_terminal(terminal, None, status)
    }

    /// Herdr fails every start of an agent with this reason.
    pub fn failing_to_start(self, reason: &str) -> Self {
        *self.start_refusal.lock().unwrap() = Some(reason.to_owned());
        self
    }

    /// The terminals that carry the review label of the subject now.
    pub fn review_agents_of(&self, subject: &str) -> Vec<String> {
        let subject: ReviewSubject = subject.parse().unwrap();
        self.reviewing
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, labelled)| **labelled == subject)
            .map(|(terminal, _)| terminal.to_string())
            .collect()
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

    /// Herdr refuses to type into the terminal until `settled` is called.
    pub fn still_starting(&self, terminal: &str) {
        self.starting
            .lock()
            .unwrap()
            .push(TerminalId::new(terminal));
    }

    pub fn settled(&self, terminal: &str) {
        self.starting
            .lock()
            .unwrap()
            .retain(|starting| starting.as_str() != terminal);
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

    /// A live terminal running Claude in a folder; it can be the change set's agent of a local review.
    pub fn claude_in(self, terminal: &str, cwd: &str) -> Self {
        self.running_in(terminal, true, "herdr", cwd)
    }

    /// A live terminal running a plain shell in a folder.
    pub fn shell_in(self, terminal: &str, cwd: &str) -> Self {
        self.running_in(terminal, false, "herdr", cwd)
    }

    fn running(self, terminal: &str, runs_claude: bool, space: &str) -> Self {
        self.running_in(terminal, runs_claude, space, "/")
    }

    fn running_in(self, terminal: &str, runs_claude: bool, space: &str, cwd: &str) -> Self {
        self.live.lock().unwrap().push(LiveTerminal {
            terminal: TerminalId::new(terminal),
            runs_claude,
            space: space.to_owned(),
            cwd: PathBuf::from(cwd),
        });
        self.with_terminal(terminal, None, AgentStatus::Unreported)
    }

    /// After the next send, each look at the terminal's status reports the next of these.
    pub fn after_a_send_looks_report(self, statuses: &[AgentStatus]) -> Self {
        *self.after_send.lock().unwrap() = statuses.to_vec();
        self
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
        let pr: ReviewSubject = pr.parse().unwrap();
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
    fn link(&self) -> Result<(), AgentLinkOff> {
        self.link.lock().unwrap().clone()
    }

    fn terminals_of(&self, pr: &ReviewSubject) -> anyhow::Result<Vec<TerminalId>> {
        if let Some(reason) = self.refusing.lock().unwrap().clone() {
            return Err(HerdrRefused {
                command: "ls --json".to_owned(),
                reason,
            }
            .into());
        }
        Ok(self
            .terminals
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, (labelled, _))| labelled.as_ref() == Some(pr))
            .map(|(terminal, _)| terminal.clone())
            .collect())
    }

    fn review_terminals_of(&self, subject: &ReviewSubject) -> anyhow::Result<Vec<TerminalId>> {
        Ok(self
            .reviewing
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, labelled)| *labelled == subject)
            .map(|(terminal, _)| terminal.clone())
            .collect())
    }

    fn live_terminals(&self) -> anyhow::Result<Vec<LiveTerminal>> {
        Ok(self.live.lock().unwrap().clone())
    }

    fn claim(&self, terminal: &TerminalId, pr: &ReviewSubject) -> anyhow::Result<()> {
        self.record(TourAgentCall::Claim {
            terminal: terminal.clone(),
            subject: pr.clone(),
        });
        let mut terminals = self.terminals.lock().unwrap();
        let (labelled, _) = terminals
            .get_mut(terminal)
            .ok_or_else(|| anyhow::anyhow!("no terminal {terminal}"))?;
        *labelled = Some(pr.clone());
        Ok(())
    }

    fn release(&self, terminal: &TerminalId, _subject: &ReviewSubject) -> anyhow::Result<()> {
        self.record(TourAgentCall::Release(terminal.clone()));
        if let Some((labelled, _)) = self.terminals.lock().unwrap().get_mut(terminal) {
            *labelled = None;
        }
        Ok(())
    }

    fn claim_review(&self, terminal: &TerminalId, subject: &ReviewSubject) -> anyhow::Result<()> {
        self.record(TourAgentCall::ClaimReview {
            terminal: terminal.clone(),
            subject: subject.clone(),
        });
        self.reviewing
            .lock()
            .unwrap()
            .insert(terminal.clone(), subject.clone());
        Ok(())
    }

    fn release_review(
        &self,
        terminal: &TerminalId,
        _subject: &ReviewSubject,
    ) -> anyhow::Result<()> {
        self.record(TourAgentCall::ReleaseReview(terminal.clone()));
        self.reviewing.lock().unwrap().remove(terminal);
        Ok(())
    }

    fn status(&self, terminal: &TerminalId) -> anyhow::Result<AgentStatus> {
        let mut looks = self.looks.lock().unwrap();
        let scripted = if looks.len() > 1 {
            looks.pop_front()
        } else {
            looks.front().copied()
        };
        if let Some(status) = scripted {
            return Ok(status);
        }
        drop(looks);
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
        if self.starting.lock().unwrap().contains(terminal) {
            return Ok(SendOutcome::Starting);
        }
        let mut terminals = self.terminals.lock().unwrap();
        let (_, status) = terminals
            .get_mut(terminal)
            .ok_or_else(|| anyhow::anyhow!("no terminal {terminal}"))?;
        if *status == AgentStatus::Unreported {
            return Ok(SendOutcome::Unreported(terminal.clone()));
        }
        if *status != AgentStatus::Idle {
            return Ok(SendOutcome::Refused(format!(
                "{terminal} is {status}, not idle"
            )));
        }
        drop(terminals);
        self.send(terminal, text)?;
        Ok(SendOutcome::Delivered)
    }

    fn send(&self, terminal: &TerminalId, text: &str) -> anyhow::Result<()> {
        let mut terminals = self.terminals.lock().unwrap();
        let (_, status) = terminals
            .get_mut(terminal)
            .ok_or_else(|| anyhow::anyhow!("no terminal {terminal}"))?;
        *status = AgentStatus::Working;
        drop(terminals);
        let script = std::mem::take(&mut *self.after_send.lock().unwrap());
        self.looks.lock().unwrap().extend(script);
        self.record(TourAgentCall::Send {
            terminal: terminal.clone(),
            text: text.to_owned(),
        });
        Ok(())
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
                let reviewed = self.reviewing.lock().unwrap().get(terminal).cloned();
                Some(AppAgent {
                    terminal: terminal.clone(),
                    role: if reviewed.is_some() {
                        AgentRole::Review
                    } else {
                        AgentRole::Tour
                    },
                    subject: reviewed.or_else(|| pr.clone()),
                    status: *status,
                    status_since_ms: *since,
                })
            })
            .collect())
    }

    fn start(&self, start: &TourAgentStart) -> anyhow::Result<TerminalId> {
        if let Some(reason) = self.start_refusal.lock().unwrap().clone() {
            anyhow::bail!("{reason}");
        }
        self.record(TourAgentCall::Start {
            name: start.name.clone(),
            subject: start.subject.clone(),
            folder: start.folder.clone(),
            first_task: start.first_task.clone(),
        });
        let mut terminals = self.terminals.lock().unwrap();
        let terminal = TerminalId::new(format!("t{}", 100 + terminals.len()));
        terminals.insert(
            terminal.clone(),
            (Some(start.subject.clone()), AgentStatus::Working),
        );
        self.started_by_app
            .lock()
            .unwrap()
            .insert(terminal.clone(), u64::MAX);
        drop(terminals);
        let script = std::mem::take(&mut *self.after_send.lock().unwrap());
        self.looks.lock().unwrap().extend(script);
        Ok(terminal)
    }

    fn start_review(&self, start: &ReviewAgentStart) -> anyhow::Result<TerminalId> {
        if let Some(reason) = self.start_refusal.lock().unwrap().clone() {
            anyhow::bail!("{reason}");
        }
        self.record(TourAgentCall::StartReview {
            name: start.name.clone(),
            subject: start.subject.clone(),
            folder: start.place.folder().to_path_buf(),
            first_task: start.first_task.clone(),
            tools: review_agent_tools(&start.place, &start.findings, start.skill.as_deref()),
            denied_tools: review_agent_denied_tools(),
        });
        let mut terminals = self.terminals.lock().unwrap();
        let terminal = TerminalId::new(format!("t{}", 100 + terminals.len()));
        terminals.insert(terminal.clone(), (None, AgentStatus::Working));
        self.reviewing
            .lock()
            .unwrap()
            .insert(terminal.clone(), start.subject.clone());
        self.started_by_app
            .lock()
            .unwrap()
            .insert(terminal.clone(), u64::MAX);
        Ok(terminal)
    }

    fn kill(&self, terminal: &TerminalId) -> anyhow::Result<()> {
        self.record(TourAgentCall::Kill(terminal.clone()));
        self.reviewing.lock().unwrap().remove(terminal);
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
