use super::tour_agent::{AgentStatus, TerminalId};

/// A tour agent writing a PR's tour, as the loading screen and the PR switcher show it. Only its `open`
/// of the PR says the tour is done; the tour file appearing does not, as it is written before it is checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TourWriting {
    pub stage: WritingStage,
    pub terminal: Option<TerminalId>,
    pub status: Option<AgentStatus>,
    /// The agent's last screen lines, top first.
    pub screen: Vec<String>,
    /// The agent took its first turn, so going idle now means it stopped, not that it is starting up.
    has_worked: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WritingStage {
    Starting,
    Writing,
    /// It ended without a tour, for this reason.
    Failed(String),
}

impl TourWriting {
    pub(super) fn starting() -> Self {
        Self {
            stage: WritingStage::Starting,
            terminal: None,
            status: None,
            screen: Vec::new(),
            has_worked: false,
        }
    }

    pub fn has_failed(&self) -> bool {
        matches!(self.stage, WritingStage::Failed(_))
    }

    pub(super) fn started(&mut self, started: anyhow::Result<TerminalId>) {
        match started {
            Ok(terminal) => {
                self.terminal = Some(terminal);
                self.stage = WritingStage::Writing;
            }
            Err(error) => {
                self.stage =
                    WritingStage::Failed(format!("The tour agent could not start: {error:#}"));
            }
        }
    }

    /// Found again after a restart. It started before, so going idle now means it stopped.
    pub(super) fn resumed(&mut self, terminal: TerminalId, status: Option<AgentStatus>) {
        self.terminal = Some(terminal);
        self.stage = WritingStage::Writing;
        self.has_worked = true;
        if let Some(status) = status {
            self.status_is(status);
        }
    }

    pub(super) fn lost_in_restart(&mut self, reason: String) {
        self.stage = WritingStage::Failed(reason);
    }

    pub(super) fn status_is(&mut self, status: AgentStatus) {
        self.status = Some(status);
        if self.stage != WritingStage::Writing {
            return;
        }
        match status {
            AgentStatus::Working => self.has_worked = true,
            AgentStatus::Idle if self.has_worked => {
                self.stage = WritingStage::Failed(
                    "The tour agent stopped without a tour. Its last lines are below; show the agent to \
                     answer it, or try again."
                        .to_owned(),
                );
            }
            AgentStatus::Failed => {
                self.stage = WritingStage::Failed(
                    "The tour agent reported an error without a tour.".to_owned(),
                );
            }
            AgentStatus::Idle | AgentStatus::WaitingForYou | AgentStatus::Unreported => {}
        }
    }

    pub(super) fn screen_is(&mut self, lines: Vec<String>) {
        self.screen = lines;
    }

    pub(super) fn gone(&mut self) {
        if matches!(self.stage, WritingStage::Writing | WritingStage::Starting) {
            self.stage =
                WritingStage::Failed("The tour agent's terminal ended without a tour.".to_owned());
        }
    }
}
