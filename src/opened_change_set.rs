use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Context;

use crate::app_folders::AppFolders;
use crate::cached_code_source::CachedCodeSource;
use crate::code_source::CodeSource;
use crate::local_facts::LocalFacts;
use crate::local_git::LocalGit;
use crate::local_work_tree::{LocalWorkTree, snapshot_ref};
use crate::make_basic_tour::make_basic_tour;
use crate::opened_pr::CodeAtTips;
use crate::repin::Repin;
use crate::review_subject::{LOCAL_FILES_FOLDER, LocalChangeSet, ReviewSubject, WorkTreeRoot};
use crate::tour::{Tour, TourPin, ToursOnDisk};
use crate::viewer::{PickCodeSource, ReviewStateFile, repin_tour};

/// Deletes the snapshot refs, in the clones of the local reviews on disk, of local reviews that are
/// on disk no more, as after a crash between deleting a review's files and its ref. PR fetch refs are
/// never touched. Runs when the app starts; a clone it cannot read keeps its refs.
pub fn clean_snapshot_refs(folders: &AppFolders) {
    let mut kept_by_clone: HashMap<WorkTreeRoot, HashSet<String>> = HashMap::new();
    for change_set in local_reviews_on_disk(folders) {
        kept_by_clone
            .entry(change_set.folder().clone())
            .or_default()
            .insert(snapshot_ref(&change_set));
    }
    for (folder, kept) in kept_by_clone {
        let work_tree = LocalWorkTree::at(folder);
        let refs = match work_tree.snapshot_refs() {
            Ok(refs) => refs,
            Err(error) => {
                eprintln!("could not clean the snapshot refs: {error}");
                continue;
            }
        };
        for stale in refs.iter().filter(|name| !kept.contains(*name)) {
            if let Err(error) = work_tree.delete_snapshot_ref(stale) {
                eprintln!("could not delete the snapshot ref {stale}: {error}");
            }
        }
    }
}

/// The change sets whose tours are on disk, basic or guided. A file that cannot be read names none.
fn local_reviews_on_disk(folders: &AppFolders) -> HashSet<LocalChangeSet> {
    let local_tours = Tour::tours_dir(folders).join(LOCAL_FILES_FOLDER);
    let Ok(clones) = std::fs::read_dir(local_tours) else {
        return HashSet::new();
    };
    clones
        .filter_map(|clone| std::fs::read_dir(clone.ok()?.path()).ok())
        .flatten()
        .filter_map(|tour| {
            let path = tour.ok()?.path();
            if path.extension()? != "json" {
                return None;
            }
            match Tour::load(&path).ok()?.pin().subject().ok()? {
                ReviewSubject::Local(change_set) => Some(change_set),
                ReviewSubject::PullRequest(_) => None,
            }
        })
        .collect()
}

/// A file or folder that was never there is already gone.
fn remove_if_there(path: &Path, removed: std::io::Result<()>) -> anyhow::Result<()> {
    match removed {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).with_context(|| format!("could not delete {}", path.display())),
    }
}

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
        self.refuse_when_on_another_branch()?;
        let live = self.work_tree.change_set()?;
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

    /// Update: re-pins the tour on show to a new snapshot of the folder, and only then moves the snapshot
    /// ref to it, so the ref never lets go of the commit the tour reads. Refused while the folder is on
    /// another branch, as the review is of its own branch.
    pub fn repin_to_new_snapshot(&self) -> anyhow::Result<(Tour, Repin)> {
        let subject = self.subject();
        anyhow::ensure!(
            ToursOnDisk::of(&self.folders, &subject) != ToursOnDisk::None,
            "no tour for {subject}; run `guided-review open <folder>` first"
        );
        self.refuse_when_on_another_branch()?;
        let snapshot = self.work_tree.make_snapshot()?;
        let tips = self.work_tree.tips(&snapshot)?;
        let source = LocalGit::at_worktree(self.change_set.folder());
        let base = source.merge_base(&tips.base_branch_tip, &tips.head)?;
        let repinned = repin_tour(
            &Tour::shown_path_for(&self.folders, &subject),
            &self.state_file(),
            &source,
            &base,
            &tips.head,
        )?;
        self.work_tree.keep_snapshot(&self.change_set, &snapshot)?;
        Ok(repinned)
    }

    /// Drop review: the tours, the tour writer's files and the state file go, then the snapshot ref, so
    /// a failure part way leaves at worst a ref that the clean-up on the next start takes.
    pub fn drop_review(&self) -> anyhow::Result<()> {
        let subject = self.subject();
        for tour in [
            Tour::basic_path_for(&self.folders, &subject),
            Tour::path_for(&self.folders, &subject),
        ] {
            remove_if_there(&tour, std::fs::remove_file(&tour))?;
        }
        let tour_files = Tour::files_for(&self.folders, &subject);
        remove_if_there(&tour_files, std::fs::remove_dir_all(&tour_files))?;
        self.state_file().delete()?;
        Ok(self.work_tree.forget_snapshot(&self.change_set)?)
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
        let tour = Tour::load_of(&Tour::shown_path_for(&self.folders, &subject), &subject)?;
        let source: Arc<dyn CodeSource> = Arc::new(LocalGit::at_worktree(self.change_set.folder()));
        Ok(CodeAtTips {
            tips: tour.pin().pinned_tips(),
            source: Box::new(CachedCodeSource::wrapping(source)),
        })
    }
}
