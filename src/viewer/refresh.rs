use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;

use crate::coverage;
use crate::pieces::ChangedFile;
use crate::pull_request::{PullRequestTips, PullRequestUrl, Sha};
use crate::repin::RepinStatus;
use crate::tour::Tour;

use super::new_commits::{PickCodeSource, UpdatedTour};
use super::ports::TourWriter;
use super::review::{Review, StepId};
use super::review_state::{RepinRecord, ReviewStateFile};

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
    pub unplaced_new_pieces: Vec<String>,
    pub new_or_changed_pieces: usize,
    pub pieces: usize,
}

impl RefreshBrief {
    pub fn of(tour: &Tour, files: &[ChangedFile], record: Option<&RepinRecord>) -> Self {
        let Some(record) = record.filter(|record| record.to_head == tour.head) else {
            return Self::default();
        };
        let stale_steps = tour
            .steps()
            .into_iter()
            .filter(|step| {
                step.pieces
                    .iter()
                    .flat_map(|pointer| pointer.shown_parts(tour, files))
                    .any(|part| {
                        record.status_of(&part.piece.id.to_string()) == Some(RepinStatus::Changed)
                    })
            })
            .map(|step| StaleStep {
                id: StepId::from(step.id.as_str()),
                title: step.title.clone(),
            })
            .collect();
        let unplaced_new_pieces = coverage::uncovered(tour, files)
            .into_iter()
            .map(|part| part.piece.id.to_string())
            .filter(|id| record.status_of(id) == Some(RepinStatus::New))
            .collect();
        let new_or_changed_pieces = record
            .pieces
            .values()
            .filter(|status| matches!(status, RepinStatus::New | RepinStatus::Changed))
            .count();
        Self {
            stale_steps,
            unplaced_new_pieces,
            new_or_changed_pieces,
            pieces: record.pieces.len(),
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
        if !self.stale_steps.is_empty() {
            writeln!(f, "stale steps:")?;
            for step in &self.stale_steps {
                writeln!(f, "  {}  {}", step.id, step.title)?;
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

/// The Refresh button's work: the local AI rewrites the marked steps, then a fresh `Review` opens on the result.
pub struct TourRefresher {
    pub home: PathBuf,
    pub pr: PullRequestUrl,
    pub pick_code_source: PickCodeSource,
    pub writer: Arc<dyn TourWriter>,
}

impl TourRefresher {
    /// Blocks until the AI is done; the app runs it off the main thread.
    pub fn run(&self) -> anyhow::Result<UpdatedTour> {
        self.writer.refresh(&self.pr)?;
        let tour = Tour::load(&Tour::path_for(&self.home, &self.pr))?;
        let pinned = PullRequestTips {
            base_branch_tip: Sha::new(&tour.base),
            head: Sha::new(&tour.head),
        };
        let source = (self.pick_code_source)(&pinned)?;
        let review = Review::open(
            tour,
            source.as_ref(),
            ReviewStateFile::for_pr(&self.home, &self.pr),
        )?;
        Ok(UpdatedTour {
            review,
            source: source.into(),
        })
    }
}
