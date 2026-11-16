use std::path::Path;
use std::sync::Arc;

use crate::app_folders::AppFolders;
use crate::code_source::CodeSource;
use crate::local_facts::LocalFacts;
use crate::opened_change_set::OpenedChangeSet;
use crate::pull_request::{ChangeTips, Sha};
use crate::repin::{Repin, repin};
use crate::review_subject::{LocalChangeSet, ReviewSubject};
use crate::tour::{GuidedTour, Tour, TourPin};

use super::ports::ChangeTipsPort;
use super::review::Review;
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
    Ok((tour, result))
}

/// What a local review's overview shows for the snapshot a tour is pinned to; a PR has none. A review
/// rebuilt from its tour file must be given these again, as they are not saved.
pub(super) fn local_facts_of(
    folders: &AppFolders,
    subject: &ReviewSubject,
    pin: &TourPin,
) -> anyhow::Result<Option<LocalFacts>> {
    subject
        .local()
        .map(|change_set| OpenedChangeSet::new(change_set, folders).facts(pin))
        .transpose()
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
        let facts = local_facts_of(&self.folders, &self.subject, &tour.pin)?;
        let mut review = Review::open(
            tour,
            source.as_ref(),
            ReviewStateFile::for_subject(&self.folders, &self.subject),
        )?;
        if let Some(facts) = facts {
            review.local_facts_are(facts);
        }
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
        let pr = match &self.subject {
            ReviewSubject::PullRequest(pr) => pr,
            ReviewSubject::Local(change_set) => return self.run_local(change_set),
        };
        let tips = github.pull_request_tips(pr)?;
        let source = (self.pick_code_source)(&tips)?;
        let base = source.merge_base(&tips.base_branch_tip, &tips.head)?;
        let state_file = ReviewStateFile::for_subject(&self.folders, &self.subject);
        let (tour, _) = repin_tour(
            &self.subject,
            &Tour::shown_path_for(&self.folders, &self.subject),
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

    /// Update of a local review: the tour moves to a new snapshot of the folder, and the overview's
    /// facts are read again for it, as a commit may have made uncommitted files committed.
    fn run_local(&self, change_set: &LocalChangeSet) -> anyhow::Result<RepinnedTour> {
        let opened = OpenedChangeSet::new(change_set, &self.folders);
        let (tour, _) = opened.repin_to_new_snapshot()?;
        let source = (self.pick_code_source)(&tour.pin().pinned_tips())?;
        let facts = opened.facts(tour.pin())?;
        let mut review = match tour {
            Tour::Basic(tour) => Review::open_basic(tour, source.as_ref(), opened.state_file())?,
            Tour::Guided(tour) => Review::open(tour, source.as_ref(), opened.state_file())?,
        };
        review.local_facts_are(facts);
        Ok(RepinnedTour {
            review,
            source: source.into(),
        })
    }
}
