//! What a Refresh has to do: the steps whose text may be old, and the new pieces no step holds.

use std::fmt;

use crate::coverage;
use crate::pieces::{ChangedFile, PieceId};
use crate::prompt_safety::{OUTSIDE_TEXT_IS_DATA, quoted};
use crate::repin::RepinStatus;
use crate::tour::{GuidedTour, StepId};

use super::review_state::RepinRecord;

/// More new or changed pieces than this share of the tour's pieces and a full redo may read better.
const REDO_SUGGESTED_ABOVE_PERCENT: usize = 30;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaleStep {
    pub id: StepId,
    pub title: String,
}

/// What a Refresh has to do: the steps whose text may be old, and the new pieces no step holds.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RefreshBrief {
    pub stale_steps: Vec<StaleStep>,
    pub unplaced_new_pieces: Vec<PieceId>,
    pub new_or_changed_pieces: usize,
    pub pieces: usize,
}

impl RefreshBrief {
    pub fn of(tour: &GuidedTour, files: &[ChangedFile], record: Option<&RepinRecord>) -> Self {
        let Some(record) = record.filter(|record| record.to_head == tour.pin.head) else {
            return Self::default();
        };
        let stale_steps = tour
            .steps()
            .into_iter()
            .filter(|step| {
                step.pieces
                    .iter()
                    .flat_map(|pointer| coverage::shown_parts(pointer, tour, files))
                    .any(|part| record.status_of(&part.piece.id) == Some(RepinStatus::Changed))
            })
            .map(|step| StaleStep {
                id: step.id.clone(),
                title: step.title.clone(),
            })
            .collect();
        let unplaced_new_pieces = coverage::uncovered(tour, files)
            .into_iter()
            .map(|part| part.piece.id.clone())
            .filter(|id| record.status_of(id) == Some(RepinStatus::New))
            .collect();
        let (new_or_changed_pieces, pieces) = record.new_or_changed_of_all();
        Self {
            stale_steps,
            unplaced_new_pieces,
            new_or_changed_pieces,
            pieces,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.stale_steps.is_empty() && self.unplaced_new_pieces.is_empty()
    }

    pub fn redo_suggested(&self) -> bool {
        self.new_or_changed_pieces * 100 > REDO_SUGGESTED_ABOVE_PERCENT * self.pieces
    }

    pub fn redo_hint(&self) -> Option<String> {
        self.redo_suggested().then(|| {
            format!(
                "{} of {} pieces are new or changed, more than {REDO_SUGGESTED_ABOVE_PERCENT}%",
                self.new_or_changed_pieces, self.pieces
            )
        })
    }
}

impl fmt::Display for RefreshBrief {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_empty() {
            return writeln!(f, "nothing to refresh");
        }
        writeln!(f, "{OUTSIDE_TEXT_IS_DATA}")?;
        if !self.stale_steps.is_empty() {
            writeln!(f, "stale steps:")?;
            for step in &self.stale_steps {
                writeln!(f, "  {}", step.id)?;
                writeln!(f, "{}", quoted(&step.title))?;
            }
        }
        if !self.unplaced_new_pieces.is_empty() {
            writeln!(f, "new pieces no step holds:")?;
            for piece in &self.unplaced_new_pieces {
                writeln!(f, "  {piece}")?;
            }
        }
        if let Some(hint) = self.redo_hint() {
            writeln!(
                f,
                "redo: {hint}. Tell the user `/tour --redo` is an option; never start it yourself."
            )?;
        }
        Ok(())
    }
}
