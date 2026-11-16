//! Moving a local review to the PR opened from its branch: a hand-over of state across two state
//! files. The PR's state is saved first and only then do the local tours, state file and snapshot ref
//! go, so a crash between the two carries again instead of losing work.

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::app_folders::AppFolders;
use crate::code_source::CodeSource;
use crate::file_path::FilePath;
use crate::make_basic_tour::make_basic_tour;
use crate::opened_change_set::OpenedChangeSet;
use crate::pieces;
use crate::pull_request::{ChangeTips, PullRequestUrl, Sha};
use crate::repin::repin;
use crate::review_subject::{LocalChangeSet, ReviewSubject};
use crate::tour::{Tour, ToursOnDisk};

use super::local_threads::MovedComment;
use super::move_offer::MoveOffer;
use super::ports::ChangeTipsPort;
use super::review_state::{RepinRecord, ReviewStateFile};
use super::switch_to_guided::{CarriedReads, ReadCarrier};

/// Finds the code source that reads the PR's commits, fetching what is missing.
pub type PickPullCodeSource =
    Arc<dyn Fn(&PullRequestUrl, &ChangeTips) -> anyhow::Result<Box<dyn CodeSource>> + Send + Sync>;

/// What a local review hands to the PR it moves to, worked out before anything is saved.
pub(super) struct MovedReview {
    /// The PR's head the move was worked out for, which the drafts' lines are at.
    pub(super) head: Sha,
    pub(super) file_ticks: BTreeMap<FilePath, Sha>,
    pub(super) file_read_before: BTreeMap<FilePath, Sha>,
    pub(super) steps: CarriedReads,
    pub(super) comments: Vec<MovedComment>,
}

/// The move of one local review to a PR, as the app runs it.
pub struct PrMover {
    pub folders: AppFolders,
    pub change_set: LocalChangeSet,
    pub pick_code_source: PickPullCodeSource,
}

impl PrMover {
    /// The move of `change_set`, reading the PR's commits from the change set's own folder.
    pub fn for_change_set(folders: &AppFolders, change_set: &LocalChangeSet) -> Self {
        let folder = change_set.folder().as_path().to_path_buf();
        Self {
            folders: folders.clone(),
            change_set: change_set.clone(),
            pick_code_source: Arc::new(move |pr, tips| {
                crate::pick_code_source::pick_code_source(
                    &crate::github::GitHub::from_env(),
                    pr,
                    tips,
                    &folder,
                )
            }),
        }
    }

    /// Blocks on git and on GitHub; the app runs it off the main thread. Nothing is posted: the open
    /// comments become drafts the reviewer sends or drops.
    pub fn run(&self, offer: &MoveOffer, github: &dyn ChangeTipsPort) -> anyhow::Result<()> {
        self.carry(offer, github)?;
        self.forget_local()
    }

    /// Everything of the local review that the PR's review takes, saved in the PR's state file. The
    /// local review is not touched, so a crash after this and before `forget_local` carries again.
    pub fn carry(&self, offer: &MoveOffer, github: &dyn ChangeTipsPort) -> anyhow::Result<()> {
        let pr = ReviewSubject::from(offer.pr.clone());
        self.have_a_tour_of(offer, &pr, github)?;
        let pr_tour = Tour::load(&Tour::shown_path_for(&self.folders, &pr))?;
        let source = (self.pick_code_source)(&offer.pr, &pr_tour.pin().pinned_tips())?;
        let moved = self.worked_out(&pr_tour, source.as_ref())?;
        ReviewStateFile::for_subject(&self.folders, &pr)
            .update(|state| state.take_moved_review(&moved))
    }

    /// Only once the PR's state is saved: a crash before this carries again.
    pub fn forget_local(&self) -> anyhow::Result<()> {
        OpenedChangeSet::new(&self.change_set, &self.folders).drop_review()
    }

    /// The PR may have no tour yet, as when it was opened a moment ago from another machine.
    fn have_a_tour_of(
        &self,
        offer: &MoveOffer,
        pr: &ReviewSubject,
        github: &dyn ChangeTipsPort,
    ) -> anyhow::Result<()> {
        if ToursOnDisk::of(&self.folders, pr) != ToursOnDisk::None {
            return Ok(());
        }
        let tips = github.pull_request_tips(&offer.pr)?;
        let source = (self.pick_code_source)(&offer.pr, &tips)?;
        make_basic_tour(&self.folders, pr, source.as_ref(), &tips, &offer.title)?;
        Ok(())
    }

    /// The local review's ticks carried by the re-pin rule from its pin to the PR's, and its open
    /// threads as drafts at the PR's head.
    fn worked_out(&self, pr_tour: &Tour, source: &dyn CodeSource) -> anyhow::Result<MovedReview> {
        let pin = pr_tour.pin();
        let local = ReviewSubject::from(self.change_set.clone());
        let mut local_tour = Tour::load(&Tour::shown_path_for(&self.folders, &local))?;
        let (from_base, from_head) = (local_tour.pin().base.clone(), local_tour.pin().head.clone());
        let repinned = repin(&mut local_tour, source, &pin.base, &pin.head)?;
        let mut local_state = ReviewStateFile::for_subject(&self.folders, &local).load()?;
        local_state.record_repin(
            RepinRecord::new(&from_base, &from_head, &pin.base, &pin.head, &repinned),
            &repinned,
        );
        let files = pieces::cut(source, &pin.base, &pin.head)?;
        let in_the_pr = |path: &FilePath| files.iter().any(|file| file.path == *path);
        let (kept, changed) = local_state.file_reads_after_repin();
        // A file whose code is the same is read at the PR's head from here on, so its tick never needs
        // the snapshot, which nothing keeps once the local review is gone.
        let file_ticks: BTreeMap<FilePath, Sha> = kept
            .into_keys()
            .filter(|path| in_the_pr(path))
            .map(|path| (path, pin.head.clone()))
            .collect();
        let file_read_before: BTreeMap<FilePath, Sha> = changed
            .into_iter()
            .filter(|(path, _)| in_the_pr(path))
            .collect();
        let (file_ticks, file_read_before, steps) = match pr_tour {
            Tour::Basic(_) => (file_ticks, file_read_before, CarriedReads::default()),
            Tour::Guided(guided) => {
                let reads: BTreeMap<FilePath, Sha> =
                    file_ticks.into_iter().chain(file_read_before).collect();
                let heads = reads.values().cloned().collect();
                let steps = ReadCarrier::of(guided, &files, &heads, source)?.carry(&reads);
                (BTreeMap::new(), BTreeMap::new(), steps)
            }
        };
        Ok(MovedReview {
            head: pin.head.clone(),
            file_ticks,
            file_read_before,
            steps,
            comments: local_state.local_threads().moved_to(source, &pin.head),
        })
    }
}
