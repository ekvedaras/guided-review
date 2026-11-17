use std::path::Path;
use std::sync::Arc;

use crate::app_folders::AppFolders;
use crate::code_source::CodeSource;
use crate::opened_subject::update_shown_tour;
use crate::pull_request::{ChangeTips, Sha};
use crate::repin::{Repin, repin};
use crate::review_subject::ReviewSubject;
use crate::tour::{GuidedTour, Tour};

use super::local_threads::LocalThreadStore;
use super::ports::ChangeTipsPort;
use super::review::Review;
use super::review_builder::ReviewBuilder;
use super::review_state::{RepinRecord, ReviewStateFile};

/// Re-pin as both the CLI and the app run it: the tour moves to the new commits, and the
/// per-PR state learns what became of each piece, so the viewer can show it.
pub fn repin_tour(
    subject: &ReviewSubject,
    tour_path: &Path,
    state_file: &ReviewStateFile,
    source: &dyn CodeSource,
    new_base: &Sha,
    new_head: &Sha,
) -> anyhow::Result<(Tour, Repin)> {
    let mut tour = Tour::load_of(tour_path, subject)?;
    let (old_base, old_head) = (tour.pin().base.clone(), tour.pin().head.clone());
    let result = repin(&mut tour, source, new_base, new_head)?;
    tour.save(tour_path)?;
    let record = RepinRecord::new(&old_base, &old_head, new_base, new_head, &result);
    state_file.update(|state| state.record_repin(record, &result))?;
    if !subject.has_github_threads() {
        LocalThreadStore::in_file(state_file.clone()).follow_to(source, new_head)?;
    }
    Ok((tour, result))
}

/// Finds the code source that can read the given tips, fetching what is missing.
pub type PickCodeSource =
    Arc<dyn Fn(&ChangeTips) -> anyhow::Result<Box<dyn CodeSource>> + Send + Sync>;

/// The "Update tour" button's work: what the CLI's `repin` does, then a fresh `Review` on the result.
pub struct TourRepinner {
    pub folders: AppFolders,
    pub subject: ReviewSubject,
    pub pick_code_source: PickCodeSource,
}

/// Reads the tour file again at the commits it is pinned to, for a tour the agent rewrote while the app
/// shows it.
pub struct TourReloader {
    pub folders: AppFolders,
    pub subject: ReviewSubject,
    pub pick_code_source: PickCodeSource,
}

impl TourReloader {
    /// Blocks on the code source; the app runs it off the main thread.
    pub fn run(&self) -> anyhow::Result<RepinnedTour> {
        let tour =
            GuidedTour::load_of(&Tour::path_for(&self.folders, &self.subject), &self.subject)?;
        let source = (self.pick_code_source)(&tour.pin.pinned_tips())?;
        let review = ReviewBuilder::new(&self.folders, &self.subject)
            .build(Tour::Guided(tour), source.as_ref())?;
        Ok(RepinnedTour {
            review,
            source: source.into(),
        })
    }
}

pub struct RepinnedTour {
    pub review: Review,
    pub source: Arc<dyn CodeSource>,
}

impl TourRepinner {
    /// Blocks on the network and on git; the app runs it off the main thread.
    pub fn run(&self, github: &dyn ChangeTipsPort) -> anyhow::Result<RepinnedTour> {
        let (tour, source) =
            update_shown_tour(&self.subject, &self.folders, github, &self.pick_code_source)?;
        let review =
            ReviewBuilder::new(&self.folders, &self.subject).build(tour, source.as_ref())?;
        Ok(RepinnedTour {
            review,
            source: source.into(),
        })
    }
}
