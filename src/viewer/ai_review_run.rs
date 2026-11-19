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

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum AiReviewRun {
    #[default]
    NotRun,
    /// The worktree is being made and the agent started.
    Starting,
    Running(RunningReview),
    /// The run ended without findings, for this reason.
    Failed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunningReview {
    pub terminal: TerminalId,
    pub status: Option<AgentStatus>,
    /// The agent's last screen lines, top first.
    pub screen: Vec<String>,
    /// The agent took its first turn, so going idle now means it stopped, not that it is starting up.
    has_worked: bool,
}

impl AiReviewRun {
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
            Ok(terminal) => Self::Running(RunningReview::new(terminal, false)),
            Err(error) => Self::Failed(format!("The review agent could not start: {error:#}")),
        };
    }

    /// Found again after a restart. It started before, so going idle now means it stopped.
    pub(super) fn resumed(&mut self, terminal: TerminalId, status: Option<AgentStatus>) {
        *self = Self::Running(RunningReview::new(terminal, true));
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
            AgentStatus::Working => running.has_worked = true,
            AgentStatus::Idle if running.has_worked => {
                *self = Self::Failed(
                    "The review agent stopped without findings. Its last lines are below; show the agent \
                     to answer it, or try again."
                        .to_owned(),
                );
            }
            AgentStatus::Failed => {
                *self =
                    Self::Failed("The review agent reported an error without findings.".to_owned());
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
            *self = Self::Failed("The review agent's terminal ended without findings.".to_owned());
        }
    }

    pub(super) fn findings_were_read(&mut self) {
        *self = Self::NotRun;
    }

    pub(super) fn findings_cannot_be_read(&mut self, reason: &str) {
        if self.is_on() {
            *self = Self::Failed(format!(
                "The review agent's findings cannot be read: {reason}"
            ));
        }
    }
}

impl RunningReview {
    const fn new(terminal: TerminalId, has_worked: bool) -> Self {
        Self {
            terminal,
            status: None,
            screen: Vec::new(),
            has_worked,
        }
    }
}
