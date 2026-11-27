//! Refresh tour, done by the tour agent of the subject: the task is typed into it, and the refresh lasts
//! until the agent's turn ends.

use std::time::{Duration, Instant};

use anyhow::bail;

use super::cancel::Cancel;
use super::ports::RefreshWriter;
use super::tour_agent::{AgentStatus, TerminalId, TourAgents, WriterHandOver};
use crate::review_subject::ReviewSubject;

/// How long each wait lasts.
#[derive(Debug, Clone, Copy)]
pub struct Pace {
    /// Between two looks at the agent.
    pub poll: Duration,
    /// How long a busy agent is waited for before the task is given up.
    pub free_agent: Duration,
    /// How long an agent that took the task may take to start working on it.
    pub start: Duration,
}

impl Pace {
    pub const fn real() -> Self {
        Self {
            poll: Duration::from_secs(2),
            free_agent: Duration::from_secs(600),
            start: Duration::from_secs(60),
        }
    }

    /// No waiting: each look at the agent is the last one.
    pub const fn at_once() -> Self {
        Self {
            poll: Duration::ZERO,
            free_agent: Duration::ZERO,
            start: Duration::ZERO,
        }
    }
}

pub struct TourAgentRefresh {
    agents: TourAgents,
    pace: Pace,
}

impl TourAgentRefresh {
    pub const fn new(agents: TourAgents, pace: Pace) -> Self {
        Self { agents, pace }
    }
}

impl TourAgentRefresh {
    /// Hands the task over, waiting while the agent is busy.
    fn hand_over(
        &self,
        subject: &ReviewSubject,
        cancel: &Cancel,
        on_activity: &mut dyn FnMut(String),
    ) -> anyhow::Result<TerminalId> {
        let waiting_since = Instant::now();
        loop {
            match self.agents.hand_refresh_task(subject, subject)? {
                WriterHandOver::Delivered(terminal) | WriterHandOver::Started(terminal) => {
                    return Ok(terminal);
                }
                WriterHandOver::Waiting(terminal) => {
                    if waiting_since.elapsed() >= self.pace.free_agent {
                        bail!("the tour agent {terminal} was not free to take the refresh");
                    }
                    on_activity(format!("Waiting for the tour agent {terminal} to be free"));
                    cancel.pause(self.pace.poll)?;
                }
            }
        }
    }

    /// Until the agent has worked on the task and is not working any more.
    fn wait_for_its_turn(
        &self,
        terminal: &TerminalId,
        cancel: &Cancel,
        on_activity: &mut dyn FnMut(String),
    ) -> anyhow::Result<()> {
        let handed_over = Instant::now();
        let mut has_worked = false;
        let mut said_it_waits = false;
        loop {
            match self.agents.status_of(terminal)? {
                AgentStatus::Working => {
                    has_worked = true;
                    said_it_waits = false;
                }
                AgentStatus::WaitingForYou => {
                    has_worked = true;
                    if !said_it_waits {
                        on_activity(format!("The tour agent {terminal} waits for you"));
                        said_it_waits = true;
                    }
                }
                AgentStatus::Failed => {
                    bail!("the tour agent {terminal} failed while it refreshed the tour")
                }
                AgentStatus::Idle | AgentStatus::Unreported if has_worked => return Ok(()),
                AgentStatus::Idle | AgentStatus::Unreported => {
                    if handed_over.elapsed() >= self.pace.start {
                        bail!("the tour agent {terminal} did not start the refresh");
                    }
                }
            }
            cancel.pause(self.pace.poll)?;
        }
    }
}

impl RefreshWriter for TourAgentRefresh {
    fn refresh(
        &self,
        subject: &ReviewSubject,
        cancel: &Cancel,
        on_activity: &mut dyn FnMut(String),
    ) -> anyhow::Result<()> {
        cancel.check()?;
        let terminal = self.hand_over(subject, cancel, on_activity)?;
        on_activity(format!("The tour agent {terminal} is refreshing the tour"));
        self.wait_for_its_turn(&terminal, cancel, on_activity)
    }
}
