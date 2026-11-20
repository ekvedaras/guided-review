//! What the viewer knows of the guided tour of a PR the reviewer reads as a basic tour: not asked,
//! being written, ready to switch to, or failed. The banner and the actions are decided here.

use super::tour_writing::{TourWriting, WritingStage};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum GuidedTourProgress {
    #[default]
    NotAsked,
    Writing(TourWriting),
    /// Checked and opened by its writer, and not switched to.
    Ready,
    Failed(String),
}

impl GuidedTourProgress {
    pub(super) fn of_writing(writing: &TourWriting) -> Self {
        match &writing.stage {
            WritingStage::Failed(reason) => Self::Failed(reason.clone()),
            WritingStage::Starting | WritingStage::Writing => Self::Writing(writing.clone()),
        }
    }
}

/// Why a guided-tour action does nothing now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GuidedTourRefusal {
    /// The review is of a guided tour already.
    NotBasic,
    /// The agent that writes the guided tour cannot be reached.
    AgentLinkOff,
    AlreadyWriting,
    AlreadyReady,
    NothingReady,
}

impl GuidedTourRefusal {
    pub fn reason(self) -> &'static str {
        match self {
            Self::NotBasic => "This review is of a guided tour already.",
            Self::AgentLinkOff => {
                "Writing a guided tour needs the tour agent link, which is off. See the sidebar."
            }
            Self::AlreadyWriting => "A guided tour is being written.",
            Self::AlreadyReady => "The guided tour is ready. Switch to it.",
            Self::NothingReady => "No guided tour is ready yet.",
        }
    }
}

/// The lasting banner that says the guided tour is ready.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GuidedTourBanner {
    pub text: &'static str,
    pub switch_label: &'static str,
    /// Why the switch button is off, while it is.
    pub switch_refusal: Option<GuidedTourRefusal>,
}

/// The stage of a writing, for the sidebar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuidedTourStage {
    pub line: String,
    /// What the agent last printed, so a long writing is not silent.
    pub activity: Option<String>,
    pub can_try_again: bool,
}
