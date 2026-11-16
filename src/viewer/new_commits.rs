use std::path::Path;
use std::sync::Arc;

use crate::app_folders::AppFolders;
use crate::code_source::CodeSource;
use crate::pull_request::{ChangeTips, PullRequestUrl, Sha};
use crate::repin::{Repin, repin};
use crate::review_subject::ReviewSubject;
use crate::tour::{GuidedTour, Tour};

use super::ports::ChangeTipsPort;
use super::review::Review;
use super::review_state::{RepinRecord, ReviewStateFile};

/// Re-pin as both the CLI and the app run it: the tour moves to the new commits, and the
/// per-PR state learns what became of each piece, so the viewer can show it.
pub fn repin_tour(
    tour_path: &Path,
    state_file: &ReviewStateFile,
    source: &dyn CodeSource,
    new_base: &Sha,
    new_head: &Sha,
) -> anyhow::Result<(Tour, Repin)> {
    let mut tour = Tour::load(tour_path)?;
    let (old_base, old_head) = (tour.pin().base.clone(), tour.pin().head.clone());
    let result = repin(&mut tour, source, new_base, new_head)?;
    tour.save(tour_path)?;
    let record = RepinRecord::new(&old_base, &old_head, new_base, new_head, &result);
    state_file.update(|state| state.record_repin(record, &result))?;
    Ok((tour, result))
}

/// Finds the code source that can read the given tips, fetching what is missing.
pub type PickCodeSource =
    Arc<dyn Fn(&ChangeTips) -> anyhow::Result<Box<dyn CodeSource>> + Send + Sync>;

/// The "Update tour" button's work: what the CLI's `repin` does, then a fresh `Review` on the result.
pub struct TourRepinner {
    pub folders: AppFolders,
    pub pr: PullRequestUrl,
    pub pick_code_source: PickCodeSource,
}

/// Reads the tour file again at the commits it is pinned to, for a tour the agent rewrote while the app
/// shows it.
pub struct TourReloader {
    pub folders: AppFolders,
    pub pr: PullRequestUrl,
    pub pick_code_source: PickCodeSource,
}

impl TourReloader {
    /// Blocks on the code source; the app runs it off the main thread.
    pub fn run(&self) -> anyhow::Result<RepinnedTour> {
        let tour = GuidedTour::load(&Tour::path_for(
            &self.folders,
            &ReviewSubject::from(self.pr.clone()),
        ))?;
        let source = (self.pick_code_source)(&tour.pin.pinned_tips())?;
        let review = Review::open(
            tour,
            source.as_ref(),
            ReviewStateFile::for_subject(&self.folders, &ReviewSubject::from(self.pr.clone())),
        )?;
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
        let tips = github.pull_request_tips(&self.pr)?;
        let source = (self.pick_code_source)(&tips)?;
        let base = source.merge_base(&tips.base_branch_tip, &tips.head)?;
        let state_file =
            ReviewStateFile::for_subject(&self.folders, &ReviewSubject::from(self.pr.clone()));
        let (tour, _) = repin_tour(
            &Tour::shown_path_for(&self.folders, &ReviewSubject::from(self.pr.clone())),
            &state_file,
            source.as_ref(),
            &base,
            &tips.head,
        )?;
        let review = match tour {
            Tour::Basic(tour) => Review::open_basic(tour, source.as_ref(), state_file)?,
            Tour::Guided(tour) => Review::open(tour, source.as_ref(), state_file)?,
        };
        Ok(RepinnedTour {
            review,
            source: source.into(),
        })
    }
}
