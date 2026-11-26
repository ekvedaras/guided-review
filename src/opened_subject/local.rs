use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Context;

use crate::app_folders::{AppFolders, remove_if_there};
use crate::cached_code_source::CachedCodeSource;
use crate::code_source::CodeSource;
use crate::gone_local_reviews::GoneLocalReviews;
use crate::local_git::LocalGit;
use crate::local_work_tree::{LocalWorkTree, snapshot_ref};
use crate::make_basic_tour::make_basic_tour;
use crate::pull_request::Sha;
use crate::repin::{Repin, repin};
use crate::review_subject::{LocalChangeSet, ReviewSubject};
use crate::tour::{GuidedTour, Tour, TourPin, ToursOnDisk};
use crate::viewer::{PickCodeSource, ReviewStateFile, repin_tour};

use super::CodeAtTips;

/// The local adapter: a local change set's code is its folder's git, read at the snapshot the tour is
/// pinned to, and no host. It alone moves the snapshot ref, under the tour-and-ref lock.
pub struct OpenedChangeSet {
    change_set: LocalChangeSet,
    folders: AppFolders,
    work_tree: LocalWorkTree,
}

impl OpenedChangeSet {
    pub fn new(change_set: &LocalChangeSet, folders: &AppFolders) -> Self {
        Self {
            change_set: change_set.clone(),
            folders: folders.clone(),
            work_tree: LocalWorkTree::at(change_set.folder().clone()),
        }
    }

    /// The change set of the branch a folder inside a clone is on now.
    pub fn change_set_at(folder: &Path) -> anyhow::Result<LocalChangeSet> {
        Ok(LocalWorkTree::containing(folder)?.change_set()?)
    }

    pub fn state_file(&self) -> ReviewStateFile {
        ReviewStateFile::for_subject(&self.folders, &self.subject())
    }

    /// Picks the code source again for any tips, as the app does when the tour moves: a local review
    /// always reads its own folder, whose git holds the snapshot.
    pub fn code_source_picker(&self) -> PickCodeSource {
        let folder = self.change_set.folder().clone();
        Arc::new(move |_tips| {
            let source: Arc<dyn CodeSource> = Arc::new(LocalGit::at_worktree(&folder));
            Ok(Box::new(CachedCodeSource::wrapping(source)) as Box<dyn CodeSource>)
        })
    }

    pub(super) fn subject(&self) -> ReviewSubject {
        ReviewSubject::from(self.change_set.clone())
    }

    pub(super) const fn folders(&self) -> &AppFolders {
        &self.folders
    }

    pub fn tour_path(&self) -> PathBuf {
        Tour::path_for(&self.folders, &self.subject())
    }

    /// Makes the change set's basic tour now, from a new snapshot, unless a tour is on disk already; true
    /// when made. A review that exists opens as it is, so reopening never takes a second snapshot.
    pub fn make_basic_tour_if_no_tour(&self) -> anyhow::Result<bool> {
        let _lock = self.lock_tour_and_ref()?;
        if ToursOnDisk::of(&self.folders, &self.subject()) != ToursOnDisk::None {
            return Ok(false);
        }
        self.refuse_when_on_another_branch()?;
        let live = self.work_tree.change_set()?;
        self.work_tree.refuse_when_nothing_to_review(&live)?;
        let snapshot = self.work_tree.make_snapshot()?;
        self.work_tree.keep_snapshot(&live, &snapshot)?;
        let tips = self.work_tree.tips(&snapshot)?;
        make_basic_tour(
            &self.folders,
            &ReviewSubject::from(live.clone()),
            &LocalGit::at_worktree(live.folder()),
            &tips,
            live.branch().as_str(),
        )?;
        GoneLocalReviews::in_folders(&self.folders).forget(&live)?;
        Ok(true)
    }

    /// Update: re-pins the tour on show to a new snapshot of the folder, and only then moves the snapshot
    /// ref to it, so the ref never lets go of the commit the tour reads. Refused while the folder is on
    /// another branch, as the review is of its own branch.
    pub fn repin_to_new_snapshot(&self) -> anyhow::Result<(Tour, Repin)> {
        let subject = self.subject();
        let _lock = self.lock_tour_and_ref()?;
        anyhow::ensure!(
            ToursOnDisk::of(&self.folders, &subject) != ToursOnDisk::None,
            "no tour for {subject}; run `guided-review open <folder>` first"
        );
        self.refuse_when_on_another_branch()?;
        let snapshot = self.work_tree.make_snapshot()?;
        let tips = self.work_tree.tips(&snapshot)?;
        let source = LocalGit::at_worktree(self.change_set.folder());
        let base = source.merge_base(&tips.base_branch_tip, &tips.head)?;
        self.repin_waiting_guided_tour(&source, &base, &tips.head)?;
        let repinned = repin_tour(
            &self.folders,
            &subject,
            &Tour::shown_path_for(&self.folders, &subject),
            &self.state_file(),
            &source,
            &base,
            &tips.head,
        )?;
        self.work_tree.keep_snapshot(&self.change_set, &snapshot)?;
        Ok(repinned)
    }

    /// The guided tour that waits beside the shown basic one is pinned to a snapshot too, and the ref
    /// keeps only the newest: it moves with the shown tour, so no tour on disk reads a snapshot `git gc`
    /// may take. It carries no state of its own (ticks and threads follow the shown tour), and a tour the
    /// writer has not had pinned by `check` yet reads no snapshot, so it stays.
    fn repin_waiting_guided_tour(
        &self,
        source: &LocalGit,
        base: &Sha,
        head: &Sha,
    ) -> anyhow::Result<()> {
        let subject = self.subject();
        if ToursOnDisk::of(&self.folders, &subject) != ToursOnDisk::BasicWithGuided {
            return Ok(());
        }
        let path = Tour::path_for(&self.folders, &subject);
        let mut guided = Tour::Guided(GuidedTour::load_of(&path, &subject)?);
        if !guided.pin().is_pinned() {
            return Ok(());
        }
        repin(&mut guided, source, base, head)?;
        guided.save(&path)
    }

    /// Drop review: the snapshot ref goes first, then the tours, the tour writer's files and the state
    /// file. The ref first, as the clean-up on start finds a clone only through a local tour still in
    /// it: a drop that stops part way leaves files to drop again, never a ref nothing will delete.
    pub fn drop_review(&self) -> anyhow::Result<()> {
        let subject = self.subject();
        let _lock = self.lock_tour_and_ref()?;
        GoneLocalReviews::in_folders(&self.folders).record(&self.change_set)?;
        self.work_tree.forget_snapshot(&self.change_set)?;
        for tour in [
            Tour::basic_path_for(&self.folders, &subject),
            Tour::path_for(&self.folders, &subject),
        ] {
            remove_if_there(&tour, std::fs::remove_file(&tour))?;
        }
        let tour_files = Tour::files_for(&self.folders, &subject);
        remove_if_there(&tour_files, std::fs::remove_dir_all(&tour_files))?;
        self.state_file().delete()?;
        // An Update waiting on the lock still holds the file, and refuses as it finds no tour.
        let lock_path = self.tour_and_ref_lock_path();
        remove_if_there(&lock_path, std::fs::remove_file(&lock_path))
    }

    /// Held by whatever moves the tour or the snapshot ref: the first open, Update in the app, `repin`
    /// in the CLI and Drop review. Without it a second Update could save its tour between the first one's tour and
    /// ref, and the ref would end on a snapshot the tour does not read.
    fn lock_tour_and_ref(&self) -> anyhow::Result<std::fs::File> {
        let path = self.tour_and_ref_lock_path();
        if let Some(folder) = path.parent() {
            std::fs::create_dir_all(folder)
                .with_context(|| format!("could not create {}", folder.display()))?;
        }
        let lock = std::fs::File::create(&path)
            .with_context(|| format!("could not create {}", path.display()))?;
        lock.lock()
            .with_context(|| format!("could not lock {}", path.display()))?;
        Ok(lock)
    }

    fn tour_and_ref_lock_path(&self) -> PathBuf {
        self.tour_path().with_extension("json.pin.lock")
    }

    fn refuse_when_on_another_branch(&self) -> anyhow::Result<()> {
        let live = self.work_tree.change_set()?;
        anyhow::ensure!(
            live == self.change_set,
            "{} is on {} now, not on {}",
            live.folder().as_path().display(),
            live.branch().as_str(),
            self.change_set.branch().as_str()
        );
        Ok(())
    }

    /// The code at the snapshot the tour on disk is pinned to, however the folder has changed since.
    pub fn at_pinned_tips(&self) -> anyhow::Result<CodeAtTips> {
        let subject = self.subject();
        anyhow::ensure!(
            ToursOnDisk::of(&self.folders, &subject) != ToursOnDisk::None,
            "no tour for {subject}; run `guided-review open <folder>` first"
        );
        self.refuse_when_gone()?;
        let tour = Tour::load_of(&Tour::shown_path_for(&self.folders, &subject), &subject)?;
        Ok(self.code_at(tour.pin()))
    }

    /// The code the shown tour reads, once its snapshot is known to be kept.
    pub(super) fn code_of_shown(&self, pin: &TourPin) -> anyhow::Result<CodeAtTips> {
        self.refuse_when_gone()?;
        Ok(self.code_at(pin))
    }

    /// A tour with no snapshot ref beside it is one of two things. After a drop or a move had deleted the
    /// review (its record says so), a tour agent may still have written one: it is no review, so it is
    /// deleted rather than shown. Otherwise the ref was lost, which the review did not do itself: the
    /// tour stays and the reviewer is told.
    fn refuse_when_gone(&self) -> anyhow::Result<()> {
        let kept = self.work_tree.snapshot_refs()?;
        if kept.contains(&snapshot_ref(&self.change_set)) {
            return Ok(());
        }
        if GoneLocalReviews::in_folders(&self.folders).is_recorded(&self.change_set)? {
            self.drop_review()?;
            anyhow::bail!(
                "the local review of {} was dropped or moved to a PR, so its tour is not kept; `guided-review open {}` starts a new one",
                self.subject(),
                self.change_set.folder().as_path().display()
            )
        }
        anyhow::bail!(
            "the snapshot of the local review of {} is lost: its ref {} is gone from the repository, so the code the tour reads may be gone too. The tour and the reviewer's notes are kept. Use Drop review on it in the PR switcher, then `guided-review open {}` starts a new review",
            self.subject(),
            snapshot_ref(&self.change_set),
            self.change_set.folder().as_path().display()
        )
    }

    /// The code at the snapshot a tour's pin names, as `refresh` reads a guided tour that waits beside
    /// the basic one.
    pub fn code_at(&self, pin: &TourPin) -> CodeAtTips {
        let source: Arc<dyn CodeSource> = Arc::new(LocalGit::at_worktree(self.change_set.folder()));
        CodeAtTips {
            tips: pin.pinned_tips(),
            source: Box::new(CachedCodeSource::wrapping(source)),
        }
    }
}
