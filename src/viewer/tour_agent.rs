use std::fmt;
use std::ops::ControlFlow;
use std::sync::Arc;

use anyhow::{Context, bail};

use crate::pull_request::PullRequestUrl;

/// A herdr terminal, as `hc` names it (`t7`).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct TerminalId(String);

impl TerminalId {
    pub fn new(address: impl Into<String>) -> Self {
        Self(address.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for TerminalId {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// What a tour agent is doing, as herdr reports it for the terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentStatus {
    /// Waiting for input, or done with its last turn.
    Idle,
    Working,
    /// Blocked on a prompt only the reviewer can answer.
    WaitingForYou,
    Failed,
    /// The terminal runs a program that reports no status, such as a plain shell.
    Unreported,
}

impl AgentStatus {
    /// From the `state` herdr reports (`idle`, `working`, `done`, `blocked`, `error`), `None` when it reports none.
    pub fn from_herdr(state: Option<&str>) -> Self {
        match state {
            Some("idle" | "done") => Self::Idle,
            Some("working") => Self::Working,
            Some("blocked") => Self::WaitingForYou,
            Some("error") => Self::Failed,
            _ => Self::Unreported,
        }
    }
}

impl fmt::Display for AgentStatus {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str(match self {
            Self::Idle => "idle",
            Self::Working => "working",
            Self::WaitingForYou => "waiting for you",
            Self::Failed => "failed",
            Self::Unreported => "status unknown",
        })
    }
}

/// Herdr behind the viewer: where a PR's tour agent runs and what it does. One adapter, `hc`.
pub trait TourAgentPort: Send + Sync {
    /// Whether tour agents can be reached at all; the reason when not.
    fn link(&self) -> Result<(), String>;

    /// The live terminals labelled as the tour agent of the PR.
    fn terminals_of(&self, pr: &PullRequestUrl) -> anyhow::Result<Vec<TerminalId>>;

    /// Labels the terminal as the tour agent of the PR. Releasing the others is the caller's job.
    fn claim(&self, terminal: &TerminalId, pr: &PullRequestUrl) -> anyhow::Result<()>;

    /// Takes the PR label off the terminal.
    fn release(&self, terminal: &TerminalId) -> anyhow::Result<()>;

    fn status(&self, terminal: &TerminalId) -> anyhow::Result<AgentStatus>;

    /// Blocks, passing every status the terminal reports from now on, until herdr's stream ends or
    /// `on_status` breaks.
    fn follow(
        &self,
        terminal: &TerminalId,
        on_status: &mut dyn FnMut(AgentStatus) -> ControlFlow<()>,
    ) -> anyhow::Result<()>;

    /// Brings the terminal up in the herdr app.
    fn show(&self, terminal: &TerminalId) -> anyhow::Result<()>;
}

/// What the sidebar says of a PR's tour agent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TourAgentState {
    /// Not looked yet.
    Unknown,
    /// Tour agents cannot be reached, for this reason.
    LinkOff(String),
    None,
    Agent {
        terminal: TerminalId,
        status: AgentStatus,
    },
}

impl TourAgentState {
    pub fn can_show(&self) -> bool {
        matches!(self, Self::Agent { .. })
    }
}

impl fmt::Display for TourAgentState {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Unknown => write!(formatter, "Tour agent: looking"),
            Self::LinkOff(reason) => write!(
                formatter,
                "Tour agent is off, it needs herdr (`hc`) and its server running: {reason}"
            ),
            Self::None => write!(formatter, "Tour agent: none"),
            Self::Agent { terminal, status } => {
                write!(formatter, "Tour agent {terminal}: {status}")
            }
        }
    }
}

/// The tour agents of PRs, found by the herdr label `gr.pr=<url>`, never by a stored terminal id.
#[derive(Clone)]
pub struct TourAgents {
    port: Arc<dyn TourAgentPort>,
}

impl TourAgents {
    pub fn new(port: Arc<dyn TourAgentPort>) -> Self {
        Self { port }
    }

    /// Makes the terminal the one tour agent of the PR: every other terminal with the PR's label loses it,
    /// so a terminal reused for another PR's tour stops being this PR's agent.
    pub fn claim(&self, pr: &PullRequestUrl, terminal: &TerminalId) -> anyhow::Result<()> {
        if let Err(reason) = self.port.link() {
            bail!("tour agents are off: {reason}");
        }
        for other in self.port.terminals_of(pr)? {
            if &other != terminal {
                self.port
                    .release(&other)
                    .with_context(|| format!("could not take the label of {pr} off {other}"))?;
            }
        }
        self.port
            .claim(terminal, pr)
            .with_context(|| format!("could not make {terminal} the tour agent of {pr}"))
    }

    /// What an `open` request does with the terminal it came from: a request from a herdr terminal claims it
    /// for the PR, one from outside herdr claims nothing.
    pub fn claim_for_open(
        &self,
        pr: &PullRequestUrl,
        terminal: Option<&str>,
    ) -> anyhow::Result<()> {
        match terminal {
            Some(address) => self.claim(pr, &TerminalId::new(address)),
            None => Ok(()),
        }
    }

    pub fn lookup(&self, pr: &PullRequestUrl) -> TourAgentState {
        if let Err(reason) = self.port.link() {
            return TourAgentState::LinkOff(reason);
        }
        self.find(pr)
            .unwrap_or_else(|error| TourAgentState::LinkOff(format!("{error:#}")))
    }

    /// Passes the agent's state now and at each status change, blocking until herdr's stream ends.
    /// `on_change` breaks when nobody wants the changes any more, which ends the follow.
    pub fn follow(
        &self,
        pr: &PullRequestUrl,
        on_change: &mut dyn FnMut(TourAgentState) -> ControlFlow<()>,
    ) {
        let state = self.lookup(pr);
        let followed = match &state {
            TourAgentState::Agent { terminal, .. } => Some(terminal.clone()),
            _ => None,
        };
        if on_change(state).is_break() {
            return;
        }
        let Some(terminal) = followed else {
            return;
        };
        let followed = self.port.follow(&terminal, &mut |status| {
            on_change(TourAgentState::Agent {
                terminal: terminal.clone(),
                status,
            })
        });
        if let Err(error) = followed {
            // The stream is over either way, so there is nothing left to break.
            let _ended = on_change(TourAgentState::LinkOff(format!("{error:#}")));
        }
    }

    pub fn show(&self, pr: &PullRequestUrl) -> anyhow::Result<()> {
        match self.lookup(pr) {
            TourAgentState::Agent { terminal, .. } => self.port.show(&terminal),
            TourAgentState::LinkOff(reason) => bail!("tour agents are off: {reason}"),
            TourAgentState::None | TourAgentState::Unknown => bail!("no tour agent for {pr}"),
        }
    }

    fn find(&self, pr: &PullRequestUrl) -> anyhow::Result<TourAgentState> {
        let Some(terminal) = self.port.terminals_of(pr)?.into_iter().next() else {
            return Ok(TourAgentState::None);
        };
        let status = self.port.status(&terminal)?;
        Ok(TourAgentState::Agent { terminal, status })
    }
}
