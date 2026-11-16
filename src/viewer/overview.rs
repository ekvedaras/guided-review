//! The overview: a basic tour's first screen, in place of the guided summary.

use super::guided_tour::GuidedTourRefusal;
use super::markdown::Markdown;
use crate::file_path::FilePath;
use crate::local_facts::{LocalChange, LocalFacts};

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
    pub write_guided_tour_refusal: Option<GuidedTourRefusal>,
    /// What a local review shows where a PR shows its description; `None` for a PR, and for a local
    /// review until the app has read its clone.
    pub local: Option<LocalFacts>,
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
    pub is_read: bool,
    /// Where a local review's change to the file comes from; `None` for a PR.
    pub change: Option<LocalChange>,
}
