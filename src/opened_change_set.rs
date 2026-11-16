use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::app_folders::AppFolders;
use crate::cached_code_source::CachedCodeSource;
use crate::code_source::CodeSource;
use crate::local_facts::LocalFacts;
use crate::local_git::LocalGit;
use crate::local_work_tree::LocalWorkTree;
use crate::make_basic_tour::make_basic_tour;
use crate::opened_pr::CodeAtTips;
use crate::review_subject::{LocalChangeSet, ReviewSubject};
use crate::tour::{Tour, TourPin, ToursOnDisk};
use crate::viewer::{PickCodeSource, ReviewStateFile};

/// What every command needs to work on one local change set: where its files live and its working
/// tree. The twin of `OpenedPr`, with no GitHub in it.
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

    /// What the overview shows, as of the snapshot the tour is pinned to.
    pub fn facts(&self, pin: &TourPin) -> anyhow::Result<LocalFacts> {
        Ok(self.work_tree.facts(&self.change_set, &pin.pinned_tips())?)
    }

    fn subject(&self) -> ReviewSubject {
        ReviewSubject::from(self.change_set.clone())
    }

    pub fn tour_path(&self) -> PathBuf {
        Tour::path_for(&self.folders, &self.subject())
    }

    /// Makes the change set's basic tour now, from a new snapshot, unless a tour is on disk already; true
    /// when made. A review that exists opens as it is, so reopening never takes a second snapshot.
    pub fn make_basic_tour_if_no_tour(&self) -> anyhow::Result<bool> {
        if ToursOnDisk::of(&self.folders, &self.subject()) != ToursOnDisk::None {
            return Ok(false);
        }
        let live = self.work_tree.change_set()?;
        anyhow::ensure!(
            live == self.change_set,
            "{} is on {} now, not on {}",
            live.folder().as_path().display(),
            live.branch().as_str(),
            self.change_set.branch().as_str()
        );
        self.work_tree.refuse_when_nothing_to_review(&live)?;
        let snapshot = self.work_tree.snapshot(&live)?;
        let tips = self.work_tree.tips(&snapshot)?;
        make_basic_tour(
            &self.folders,
            &ReviewSubject::from(live.clone()),
            &LocalGit::at_worktree(live.folder()),
            &tips,
            live.branch().as_str(),
        )?;
        Ok(true)
    }

    /// The code at the snapshot the tour on disk is pinned to, however the folder has changed since.
    pub fn at_pinned_tips(&self) -> anyhow::Result<CodeAtTips> {
        let subject = self.subject();
        anyhow::ensure!(
            ToursOnDisk::of(&self.folders, &subject) != ToursOnDisk::None,
            "no tour for {subject}; run `guided-review open <folder>` first"
        );
        let tour = Tour::load_of(&Tour::shown_path_for(&self.folders, &subject), &subject)?;
        let source: Arc<dyn CodeSource> = Arc::new(LocalGit::at_worktree(self.change_set.folder()));
        Ok(CodeAtTips {
            tips: tour.pin().pinned_tips(),
            source: Box::new(CachedCodeSource::wrapping(source)),
        })
    }
}
