use super::ReviewCore;
use crate::tour::Tour;
use crate::viewer::guided_tour::{
    GuidedTourBanner, GuidedTourProgress, GuidedTourRefusal, GuidedTourStage,
};
use crate::viewer::jump::PaletteAction;
use crate::viewer::tour_agent::TourAgentState;
use crate::viewer::tour_writing::{TourWriting, WritingPhase};

impl ReviewCore {
    pub fn guided_tour_progress(&self) -> &GuidedTourProgress {
        &self.guided_tour
    }

    /// What the app knows of the guided tour now: at a build, and as its writing moves on. The
    /// screen, the page and the reading place stay as they are.
    pub fn guided_tour_is(&mut self, progress: GuidedTourProgress) {
        self.guided_tour = progress;
    }

    /// A snapshot scene's count of steps the switch from a basic tour carried read ticks to.
    #[cfg(feature = "snapshots")]
    pub fn steps_carried_read_ticks_are(&mut self, count: usize) {
        self.steps_started_read = Some(count);
    }

    pub fn steps_that_started_read(&self) -> Option<usize> {
        self.steps_started_read
    }

    pub(super) fn is_basic(&self) -> bool {
        matches!(self.tour, Tour::Basic(_))
    }

    /// `None` when Write guided tour, or Try again after a failure, can start a writing now.
    pub fn write_guided_tour_refusal(&self) -> Option<GuidedTourRefusal> {
        if !self.is_basic() {
            return Some(GuidedTourRefusal::NotBasic);
        }
        match self.guided_tour {
            GuidedTourProgress::Writing(_) => Some(GuidedTourRefusal::AlreadyWriting),
            GuidedTourProgress::Ready => Some(GuidedTourRefusal::AlreadyReady),
            GuidedTourProgress::NotAsked | GuidedTourProgress::Failed(_) => self
                .agent_link_is_off
                .then_some(GuidedTourRefusal::AgentLinkOff),
        }
    }

    /// Write guided tour and Try again: the writing starts, and the app hands the task to the agent.
    pub fn ask_to_write_guided_tour(&mut self) -> Result<(), GuidedTourRefusal> {
        if let Some(refusal) = self.write_guided_tour_refusal() {
            return Err(refusal);
        }
        self.guided_tour = GuidedTourProgress::Writing(TourWriting::starting());
        Ok(())
    }

    /// Shown from ready until the reviewer switches, in the page header and the sidebar.
    pub fn guided_tour_banner(&self) -> Option<GuidedTourBanner> {
        if !self.is_basic() || self.guided_tour != GuidedTourProgress::Ready {
            return None;
        }
        Some(GuidedTourBanner {
            text: "The guided tour is ready.",
            switch_label: "Switch to guided tour",
            switch_refusal: self.switch_to_guided_tour().err(),
        })
    }

    /// The reviewer's click on Switch to guided tour; the app builds the guided review.
    pub fn switch_to_guided_tour(&self) -> Result<(), GuidedTourRefusal> {
        if !self.is_basic() {
            return Err(GuidedTourRefusal::NotBasic);
        }
        match self.guided_tour {
            GuidedTourProgress::Ready => Ok(()),
            GuidedTourProgress::NotAsked
            | GuidedTourProgress::Writing(_)
            | GuidedTourProgress::Failed(_) => Err(GuidedTourRefusal::NothingReady),
        }
    }

    /// What the sidebar says of a writing, and whether it can be tried again.
    pub fn guided_tour_stage(&self) -> Option<GuidedTourStage> {
        if !self.is_basic() {
            return None;
        }
        match &self.guided_tour {
            GuidedTourProgress::NotAsked | GuidedTourProgress::Ready => None,
            GuidedTourProgress::Writing(writing) => Some(GuidedTourStage {
                line: match writing.phase() {
                    WritingPhase::WaitsForYou => {
                        "Guided tour: the tour agent waits for you in its terminal"
                    }
                    WritingPhase::Starting => "Guided tour: asking the tour agent to write it",
                    WritingPhase::Writing => "Guided tour: the tour agent is writing it",
                }
                .to_owned(),
                activity: writing.latest_step(),
                can_try_again: false,
            }),
            GuidedTourProgress::Failed(reason) => Some(GuidedTourStage {
                line: format!("Guided tour failed: {reason}"),
                activity: None,
                can_try_again: self.write_guided_tour_refusal().is_none(),
            }),
        }
    }

    /// While a guided tour is written its stage speaks for the agent, so the sidebar has one agent line.
    pub fn shows_tour_agent_line(&self, agent: &TourAgentState) -> bool {
        self.guided_tour_stage().is_none() && agent.is_worth_a_line()
    }

    pub(in crate::viewer) fn guided_tour_palette_actions(&self) -> Vec<PaletteAction> {
        if !self.is_basic() {
            return Vec::new();
        }
        match self.guided_tour {
            GuidedTourProgress::Ready => vec![PaletteAction::SwitchToGuidedTour],
            GuidedTourProgress::NotAsked | GuidedTourProgress::Failed(_)
                if self.write_guided_tour_refusal().is_none() =>
            {
                vec![PaletteAction::WriteGuidedTour]
            }
            GuidedTourProgress::NotAsked
            | GuidedTourProgress::Writing(_)
            | GuidedTourProgress::Failed(_) => Vec::new(),
        }
    }
}
