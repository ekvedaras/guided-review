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

/// Where a writing stands while it may still give a tour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WritingPhase {
    WaitsForYou,
    Starting,
    Writing,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WritingStage {
    Starting,
    Writing,
    /// It ended without a tour, for this reason.
    Failed(String),
}

impl TourWriting {
    pub fn starting() -> Self {
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

    /// Why it ended without a tour; `None` while it may still write one.
    pub fn failure(&self) -> Option<&str> {
        match &self.stage {
            WritingStage::Failed(reason) => Some(reason),
            WritingStage::Starting | WritingStage::Writing => None,
        }
    }

    /// Where a writing that has not failed stands, for the lines that say so. The agent waiting
    /// for the reviewer comes first, whatever its stage.
    pub fn phase(&self) -> WritingPhase {
        match (&self.stage, self.status) {
            (_, Some(AgentStatus::WaitingForYou)) => WritingPhase::WaitsForYou,
            (WritingStage::Starting, _) => WritingPhase::Starting,
            (WritingStage::Writing | WritingStage::Failed(_), _) => WritingPhase::Writing,
        }
    }

    /// No agent has taken the task yet, and the writing has not failed.
    pub fn waits_for_an_agent(&self) -> bool {
        self.terminal.is_none() && !self.has_failed()
    }

    pub fn runs_in(&self, terminal: &TerminalId) -> bool {
        self.terminal.as_ref() == Some(terminal)
    }

    /// What the loading screen says of the agent.
    pub fn status_line(&self) -> String {
        let Some(terminal) = &self.terminal else {
            return "Starting a tour agent in herdr...".to_owned();
        };
        match self.status {
            None | Some(AgentStatus::Unreported) => format!("Tour agent {terminal}: starting"),
            Some(AgentStatus::WaitingForYou) => format!(
                "Tour agent {terminal} waits for you in its terminal: show the agent to answer it"
            ),
            Some(status) => format!("Tour agent {terminal}: {status}"),
        }
    }

    /// The agent's last screen lines, top first.
    pub fn last_lines(&self) -> impl Iterator<Item = &str> {
        self.screen.iter().map(String::as_str)
    }

    /// The tour was written, but opening it failed.
    pub(super) fn failed_to_open(reason: &str) -> Self {
        let mut writing = Self::starting();
        writing.stage = WritingStage::Failed(format!(
            "The tour was written but could not be opened: {reason}"
        ));
        writing
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

    pub fn status_is(&mut self, status: AgentStatus) {
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

    /// The text of the latest step Claude printed on its screen (a line led by `•`, `⏺` or `●`). Its spinner
    /// word, tool output, prompt box and status bar are not steps.
    pub fn latest_step(&self) -> Option<String> {
        self.screen.iter().rev().find_map(|line| {
            let step = line.trim_start().strip_prefix(['•', '⏺', '●'])?.trim();
            (!step.is_empty()).then(|| step.to_owned())
        })
    }

    pub fn screen_is(&mut self, lines: Vec<String>) {
        self.screen = lines;
    }

    pub(super) fn gone(&mut self) {
        if matches!(self.stage, WritingStage::Writing | WritingStage::Starting) {
            self.stage =
                WritingStage::Failed("The tour agent's terminal ended without a tour.".to_owned());
        }
    }
}
