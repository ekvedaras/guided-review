use super::ask::Place;
use super::ports::AgentLaunch;

/// What the Ask panel is open on: a place an AI can be asked about, or one it cannot, and why.
#[derive(Debug, Clone)]
pub enum AskTarget {
    Ready {
        place: Place,
        label: String,
        context: String,
        launch: AgentLaunch,
    },
    Unavailable {
        place: Place,
        label: String,
        why: Unavailable,
    },
}

impl AskTarget {
    pub fn place(&self) -> &Place {
        match self {
            Self::Ready { place, .. } | Self::Unavailable { place, .. } => place,
        }
    }

    pub fn label(&self) -> &str {
        match self {
            Self::Ready { label, .. } | Self::Unavailable { label, .. } => label,
        }
    }
}

/// Why no new question can be asked about a place.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unavailable {
    /// Without a local clone the AI has no code to read.
    NoClone,
    PlaceGone,
}

impl Unavailable {
    pub fn message(self) -> &'static str {
        match self {
            Self::NoClone => {
                "Ask needs a local clone of this repo: run the app inside it or add it under [clones] in ~/.guided-review/config.toml"
            }
            Self::PlaceGone => "These lines are not in the tour any more: select them again to ask",
        }
    }
}
