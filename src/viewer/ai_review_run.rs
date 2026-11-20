//! The AI review of the subject the reviewer reads, as a run of the review agent: not run, starting, working,
//! or failed. A finished run is not a state: its findings file is read and the run is gone.

use super::tour_agent::{AgentStatus, TerminalId};

/// Why Run AI review does nothing now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AiReviewRefusal {
    AlreadyRunning,
    /// The agents cannot be reached.
    AgentLinkOff,
}

impl AiReviewRefusal {
    pub fn reason(self) -> &'static str {
        match self {
            Self::AlreadyRunning => "The AI review is running.",
            Self::AgentLinkOff => {
                "The AI review needs the agent link, which is off. See the sidebar."
            }
        }
    }
}

/// The button the AI review row and the overview offer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AiReviewAction {
    Run,
    /// After findings: a second run replaces the undecided ones.
    RunAgain,
    TryAgain,
}

impl AiReviewAction {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Run => "Run AI review",
            Self::RunAgain => "Run again",
            Self::TryAgain => "Try again",
        }
    }
}

/// What the sidebar's AI review row says and offers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AiReviewStage {
    pub line: String,
    pub is_failure: bool,
    pub action: Option<AiReviewAction>,
    pub can_show_agent: bool,
    /// There are findings to walk through and to list, whatever the run is doing.
    pub has_findings: bool,
    /// The findings list is there to restore them from, even with none open.
    pub has_dismissed_findings: bool,
}

/// Whether the subject's review agent is alive, as its follow last said; it outlives the run, idle, to be
/// talked to.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ReviewAgentLife {
    #[default]
    Gone,
    /// With its last status, when one came.
    Alive(Option<AgentStatus>),
}

impl ReviewAgentLife {
    pub const fn is_alive(self) -> bool {
        matches!(self, Self::Alive(_))
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum AiReviewRun {
    #[default]
    NotRun,
    /// The worktree is being made and the agent started.
    Starting,
    Running(RunningReview),
    /// The run ended without findings, for this reason. `agent` is the review agent's terminal when it is
    /// still there to show and answer.
    Failed {
        reason: String,
        agent: Option<TerminalId>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunningReview {
    pub terminal: TerminalId,
    pub status: Option<AgentStatus>,
    /// The agent's last screen lines, top first.
    pub screen: Vec<String>,
    first_turn: FirstTurn,
}

/// Whether the agent took its first turn: going idle after it means the agent stopped, before it
/// means the agent is starting up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FirstTurn {
    NotTakenYet,
    Taken,
}

impl AiReviewRun {
    /// A run the agent works in, as a snapshot scene shows it.
    #[cfg(feature = "snapshots")]
    pub fn staged_running() -> Self {
        Self::Running(RunningReview::found_again(TerminalId::new("t1")))
    }

    /// A run that ended because the agent stopped, as a snapshot scene shows it.
    #[cfg(feature = "snapshots")]
    pub fn staged_failed() -> Self {
        Self::Failed {
            reason:
                "The review agent stopped without findings. Show the agent to answer it, or try again."
                    .to_owned(),
            agent: Some(TerminalId::new("t1")),
        }
    }

    /// A run that has not ended: it holds the review agent and its worktree.
    pub const fn is_on(&self) -> bool {
        matches!(self, Self::Starting | Self::Running(_))
    }

    pub(super) fn starting() -> Self {
        Self::Starting
    }

    pub(super) fn started(&mut self, started: anyhow::Result<TerminalId>) {
        if *self != Self::Starting {
            return;
        }
        *self = match started {
            Ok(terminal) => Self::Running(RunningReview::starting(terminal)),
            Err(error) => Self::Failed {
                reason: format!("The review agent could not start: {error:#}"),
                agent: None,
            },
        };
    }

    /// Found again after a restart. It started before, so going idle now means it stopped.
    pub(super) fn resumed(&mut self, terminal: TerminalId, status: Option<AgentStatus>) {
        *self = Self::Running(RunningReview::found_again(terminal));
        if let Some(status) = status {
            self.status_is(status);
        }
    }

    pub(super) fn status_is(&mut self, status: AgentStatus) {
        let Self::Running(running) = self else {
            return;
        };
        running.status = Some(status);
        match status {
            AgentStatus::Working => running.first_turn = FirstTurn::Taken,
            AgentStatus::Idle if running.first_turn == FirstTurn::Taken => {
                *self = Self::Failed {
                    reason: "The review agent stopped without findings. Show the agent to answer it, or try again."
                        .to_owned(),
                    agent: Some(running.terminal.clone()),
                };
            }
            AgentStatus::Failed => {
                *self = Self::Failed {
                    reason: "The review agent reported an error without findings.".to_owned(),
                    agent: Some(running.terminal.clone()),
                };
            }
            AgentStatus::Idle | AgentStatus::WaitingForYou | AgentStatus::Unreported => {}
        }
    }

    pub(super) fn screen_is(&mut self, lines: Vec<String>) {
        if let Self::Running(running) = self {
            running.screen = lines;
        }
    }

    pub(super) fn gone(&mut self) {
        if self.is_on() {
            *self = Self::Failed {
                reason: "The review agent's terminal ended without findings.".to_owned(),
                agent: None,
            };
        }
    }

    pub(super) fn findings_were_read(&mut self) {
        *self = Self::NotRun;
    }

    pub(super) fn findings_cannot_be_read(&mut self, reason: &str) {
        if self.is_on() {
            let agent = match self {
                Self::Running(running) => Some(running.terminal.clone()),
                _ => None,
            };
            *self = Self::Failed {
                reason: format!("The review agent's findings cannot be read: {reason}"),
                agent,
            };
        }
    }
}

impl RunningReview {
    const fn starting(terminal: TerminalId) -> Self {
        Self::on(terminal, FirstTurn::NotTakenYet)
    }

    const fn found_again(terminal: TerminalId) -> Self {
        Self::on(terminal, FirstTurn::Taken)
    }

    const fn on(terminal: TerminalId, first_turn: FirstTurn) -> Self {
        Self {
            terminal,
            status: None,
            screen: Vec::new(),
            first_turn,
        }
    }
}
