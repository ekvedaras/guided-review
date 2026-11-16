//! The overview: a basic tour's first screen, in place of the guided summary.

use super::markdown::Markdown;
use crate::file_path::FilePath;

/// What the overview shows. The PR's title, its description as the summary page shows it, the
/// counts, and every changed file in tree order.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct OverviewPage {
    pub title: String,
    pub description: Option<Markdown>,
    pub stats: OverviewStats,
    pub files: Vec<OverviewFile>,
    /// `Some` while Write guided tour is offered but cannot be done; the window draws it disabled
    /// with this reason.
    pub write_guided_tour_refusal: Option<WriteGuidedTourRefusal>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct OverviewStats {
    pub files: usize,
    pub added_lines: usize,
    pub removed_lines: usize,
    pub open_threads: usize,
}

/// A changed file as the overview lists it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct OverviewFile {
    pub path: FilePath,
    pub added_lines: usize,
    pub removed_lines: usize,
    /// The slot of the read tick, which a basic tour gets with ticket #83; unticked until then.
    pub is_read: bool,
}

/// Why a basic tour cannot write its guided tour from the overview.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WriteGuidedTourRefusal {
    /// The button exists before the writing is wired in (ticket #86).
    NotWiredYet,
}

impl WriteGuidedTourRefusal {
    pub fn reason(self) -> &'static str {
        match self {
            Self::NotWiredYet => "Writing a guided tour from a basic review is not available yet.",
        }
    }
}
