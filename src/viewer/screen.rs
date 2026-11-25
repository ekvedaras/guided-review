//! Which screen of a tour the reviewer reads, and what a read tick says was read.

use super::ask::ShownOn;
use crate::file_path::FilePath;
use crate::tour::StepId;

/// What a read tick says was read: a guided tour's step, or a basic tour's whole file.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ReadSubject {
    Step(StepId),
    File(FilePath),
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Screen {
    Summary,
    /// A basic tour's first screen, in place of the summary.
    Overview,
    /// One changed file of a basic tour, in place of a step.
    File(FilePath),
    Step(StepId),
    /// After the last step: every change no step shows.
    SafetyNet,
}

impl Screen {
    /// What an Ask about lines of this screen records as the screen it was shown on; the
    /// summary and the overview show no lines.
    pub fn shown_on(&self) -> Option<ShownOn> {
        match self {
            Self::Step(step) => Some(ShownOn::Step { step: step.clone() }),
            Self::SafetyNet => Some(ShownOn::SafetyNet),
            Self::File(path) => Some(ShownOn::File { path: path.clone() }),
            Self::Summary | Self::Overview => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum NextScreen {
    /// Its title is markdown.
    Step {
        title: String,
    },
    /// A basic tour's next file.
    File {
        path: FilePath,
    },
    SafetyNet,
}
