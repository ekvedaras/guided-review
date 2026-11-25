use super::diff_display::DiffDisplay;
use crate::coverage::{self, PiecePart};
use crate::local_facts::LocalChange;
use crate::pieces::ChangedFile;
use crate::pull_request::Sha;
use crate::tour::{GuidedTour, Step};

use super::piece_diff::PieceDiff;
use super::threads::ThreadView;

pub struct StepPage<'a> {
    pub step: &'a Step,
    pub pieces: Vec<PieceDiff>,
    pub is_ticked: bool,
    /// Skim, ticked and gone steps show their text but keep their pieces closed.
    pub is_folded: bool,
    pub threads: Vec<ThreadView>,
    /// Pieces this step showed before the last re-pin that no longer exist.
    pub gone_pieces: usize,
    /// Nothing left to show: no pieces, no diagram, only gone pieces.
    pub is_gone: bool,
    /// A piece changed in the last re-pin, so the step's text was written for older code.
    pub text_may_be_old: bool,
    /// Set when the step was read at an older head: its pieces then show only what changed since.
    pub since_read: Option<SinceRead>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SinceRead {
    pub read_at: Sha,
    /// The reviewer asked for the whole PR diff of the step instead of the change since reading.
    pub showing_full_diff: bool,
}

impl<'a> StepPage<'a> {
    pub(super) fn of(
        step: &'a Step,
        tour: &GuidedTour,
        files: &[ChangedFile],
        display: &DiffDisplay,
        is_folded: bool,
    ) -> Self {
        let pieces = step
            .pieces
            .iter()
            .flat_map(|pointer| coverage::shown_parts(pointer, tour, files))
            .filter_map(|part| PieceDiff::of(&part, files, display))
            .collect();
        Self {
            step,
            pieces,
            is_ticked: false,
            is_folded,
            threads: Vec::new(),
            gone_pieces: 0,
            is_gone: false,
            text_may_be_old: false,
            since_read: None,
        }
    }
}

/// The changes no step shows. Computed from the tour and the diff, never stored.
pub struct SafetyNetPage {
    pub pieces: Vec<PieceDiff>,
    pub threads: Vec<ThreadView>,
}

impl SafetyNetPage {
    pub(super) fn of(tour: &GuidedTour, files: &[ChangedFile], display: &DiffDisplay) -> Self {
        Self {
            pieces: coverage::safety_net(tour, files)
                .into_iter()
                .filter_map(|part| PieceDiff::of(&part, files, display))
                .collect(),
            threads: Vec::new(),
        }
    }
}

/// One changed file of a basic tour, every piece of it whole.
pub struct FilePage {
    pub pieces: Vec<PieceDiff>,
    pub threads: Vec<ThreadView>,
    /// Set when the file was read at an older head: its pieces then show only what changed since.
    pub since_read: Option<SinceRead>,
    /// Where a local review's change to this file comes from; `None` for a PR.
    pub change: Option<LocalChange>,
}

impl FilePage {
    pub(super) fn of(file: &ChangedFile, files: &[ChangedFile], display: &DiffDisplay) -> Self {
        Self {
            pieces: file
                .pieces
                .iter()
                .filter_map(|piece| PieceDiff::of(&PiecePart::whole(piece), files, display))
                .collect(),
            threads: Vec::new(),
            since_read: None,
            change: None,
        }
    }
}
