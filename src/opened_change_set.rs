use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Context;

use crate::app_folders::{AppFolders, remove_if_there};
use crate::cached_code_source::CachedCodeSource;
use crate::code_source::CodeSource;
use crate::local_facts::LocalFacts;
use crate::local_git::LocalGit;
use crate::local_work_tree::{LocalWorkTree, SharedGitFolder, snapshot_ref};
use crate::make_basic_tour::make_basic_tour;
use crate::opened_pr::CodeAtTips;
use crate::pull_request::PullRequestUrl;
use crate::repin::Repin;
use crate::review_subject::{LOCAL_FILES_FOLDER, LocalChangeSet, ReviewSubject};
use crate::tour::{Tour, TourPin, ToursOnDisk};
use crate::viewer::{PickCodeSource, ReviewStateFile, repin_tour};

/// Deletes the snapshot refs, in the repositories of the local reviews on disk, of local reviews that
/// are on disk no more, as after a crash between deleting a review's files and its ref. Linked worktrees
/// share their repository's refs, so a repository keeps the refs of the reviews in all its worktrees.
/// PR fetch refs are never touched. Runs when the app starts. A repository it cannot read keeps its
/// refs, and so does a repository with a tour it cannot read in any of its worktrees, as that tour may
/// be the very review a ref keeps; a tour that cannot be read in a folder of no readable review keeps
/// every repository's refs. Says what it left and why.
pub fn clean_snapshot_refs(folders: &AppFolders) -> Vec<String> {
    let on_disk = LocalReviewsOnDisk::read(folders);
    let mut not_cleaned: Vec<String> = on_disk
        .unreadable
        .iter()
        .map(|unreadable| {
            format!(
                "{} cannot be read, so the snapshot refs of its repository were kept: {}",
                unreadable.path.display(),
                unreadable.reason
            )
        })
        .collect();
    if on_disk.cannot_tell_whose(folders) {
        not_cleaned.push(
            "a local tour that cannot be read is of no review on disk, so no repository's snapshot refs were cleaned"
                .to_owned(),
        );
        return not_cleaned;
    }
    let mut by_repository: HashMap<SharedGitFolder, RepositoryRefs> = HashMap::new();
    for change_set in &on_disk.change_sets {
        let work_tree = LocalWorkTree::at(change_set.folder().clone());
        let shared = match work_tree.shared_git_folder() {
            Ok(shared) => shared,
            Err(error) => {
                not_cleaned.push(format!("could not clean the snapshot refs: {error}"));
                continue;
            }
        };
        let repository = by_repository
            .entry(shared)
            .or_insert_with(|| RepositoryRefs {
                work_tree,
                kept: HashSet::new(),
                cannot_tell_all: false,
            });
        repository.kept.insert(snapshot_ref(change_set));
        repository.cannot_tell_all |= on_disk.cannot_tell_all_of(folders, change_set);
    }
    for repository in by_repository.into_values() {
        if repository.cannot_tell_all {
            continue;
        }
        let RepositoryRefs {
            work_tree, kept, ..
        } = repository;
        let refs = match work_tree.snapshot_refs() {
            Ok(refs) => refs,
            Err(error) => {
                not_cleaned.push(format!("could not clean the snapshot refs: {error}"));
                continue;
            }
        };
        for stale in refs.iter().filter(|name| !kept.contains(*name)) {
            if let Err(error) = work_tree.delete_snapshot_ref(stale) {
                not_cleaned.push(format!(
                    "could not delete the snapshot ref {stale}: {error}"
                ));
            }
        }
    }
    not_cleaned
}

/// The snapshot refs of one repository that local reviews on disk keep, from all its worktrees, since
/// they share one set of refs.
struct RepositoryRefs {
    work_tree: LocalWorkTree,
    kept: HashSet<String>,
    cannot_tell_all: bool,
}

/// The change sets whose tours are on disk, basic or guided, for a list of them. A file that cannot be
/// read names none; the clean-up, which must not take that for absent, reads `LocalReviewsOnDisk`.
pub fn local_reviews_on_disk(folders: &AppFolders) -> HashSet<LocalChangeSet> {
    LocalReviewsOnDisk::read(folders).change_sets
}

/// The local tours, or folders of them, that could not be read. Each may be the review a snapshot ref
/// keeps, so the clean-up keeps every ref of its repository, and the reviewer is told.
pub fn unreadable_local_tours(folders: &AppFolders) -> Vec<UnreadableLocalTour> {
    LocalReviewsOnDisk::read(folders).unreadable
}

/// A local tour, or a folder of them, that could not be read, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnreadableLocalTour {
    pub path: PathBuf,
    pub reason: String,
}

/// The change sets whose tours are on disk, basic or guided, and the tours and folders of local tours
/// that could not be read, which may be of any review.
struct LocalReviewsOnDisk {
    change_sets: HashSet<LocalChangeSet>,
    unreadable: Vec<UnreadableLocalTour>,
}

impl LocalReviewsOnDisk {
    fn read(folders: &AppFolders) -> Self {
        let mut on_disk = Self {
            change_sets: HashSet::new(),
            unreadable: Vec::new(),
        };
        let local_tours = Tour::tours_dir(folders).join(LOCAL_FILES_FOLDER);
        let clones = match std::fs::read_dir(&local_tours) {
            Ok(clones) => clones,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return on_disk,
            Err(error) => {
                on_disk.cannot_read(local_tours, error.to_string());
                return on_disk;
            }
        };
        for clone in clones {
            match clone.and_then(|clone| Ok((std::fs::read_dir(clone.path())?, clone.path()))) {
                Ok((tours, folder)) => on_disk.read_tours_in(&folder, tours),
                Err(error) => on_disk.cannot_read(local_tours.clone(), error.to_string()),
            }
        }
        on_disk
    }

    fn read_tours_in(&mut self, folder: &Path, tours: std::fs::ReadDir) {
        for tour in tours {
            let path = match tour {
                Ok(tour) => tour.path(),
                Err(error) => {
                    self.cannot_read(folder.to_path_buf(), error.to_string());
                    continue;
                }
            };
            if path.extension().is_none_or(|extension| extension != "json") {
                continue;
            }
            match Tour::load(&path).and_then(|tour| tour.pin().subject()) {
                Ok(ReviewSubject::Local(change_set)) => {
                    self.change_sets.insert(change_set);
                }
                Ok(ReviewSubject::PullRequest(_)) => {}
                Err(error) => self.cannot_read(path, format!("{error:#}")),
            }
        }
    }

    fn cannot_read(&mut self, path: PathBuf, reason: String) {
        self.unreadable.push(UnreadableLocalTour { path, reason });
    }

    /// Whether something unreadable may be a review of the change set's clone: a tour in its folder,
    /// or a folder holding it.
    fn cannot_tell_all_of(&self, folders: &AppFolders, change_set: &LocalChangeSet) -> bool {
        let tours_folder = Self::tours_folder_of(folders, change_set);
        self.unreadable
            .iter()
            .any(|unreadable| unreadable.may_be_in(&tours_folder))
    }

    /// Whether something unreadable is in the folder of no readable review, and so may be the review
    /// of any repository: a worktree whose only tour cannot be read names no folder to find its
    /// repository by.
    fn cannot_tell_whose(&self, folders: &AppFolders) -> bool {
        let known: Vec<PathBuf> = self
            .change_sets
            .iter()
            .map(|change_set| Self::tours_folder_of(folders, change_set))
            .collect();
        self.unreadable.iter().any(|unreadable| {
            !known
                .iter()
                .any(|tours_folder| unreadable.may_be_in(tours_folder))
        })
    }

    fn tours_folder_of(folders: &AppFolders, change_set: &LocalChangeSet) -> PathBuf {
        let subject = ReviewSubject::from(change_set.clone());
        Tour::tours_dir(folders).join(subject.files_folder())
    }
}

impl UnreadableLocalTour {
    /// A tour in the folder, or a folder holding it.
    fn may_be_in(&self, tours_folder: &Path) -> bool {
        self.path.starts_with(tours_folder) || tours_folder.starts_with(&self.path)
    }
}

/// The local reviews on disk of the PR's repo whose move to the PR was not declined: one of them may be of
/// the branch the PR is opened from. Reads the tour and state files only: no GitHub and no git.
pub fn local_reviews_of_repo(folders: &AppFolders, pr: &PullRequestUrl) -> Vec<LocalChangeSet> {
    local_reviews_on_disk(folders)
        .into_iter()
        .filter(|review| review.repo() == Some(&pr.repo))
        .filter(|review| {
            let subject = ReviewSubject::from(review.clone());
            !ReviewStateFile::for_subject(folders, &subject).has_declined_move_to(pr)
        })
        .collect()
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
        let _lock = self.lock_tour_and_ref()?;
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
        let repinned = repin_tour(
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

    /// Drop review: the snapshot ref goes first, then the tours, the tour writer's files and the state
    /// file. The ref first, as the clean-up on start finds a clone only through a local tour still in
    /// it: a drop that stops part way leaves files to drop again, never a ref nothing will delete.
    pub fn drop_review(&self) -> anyhow::Result<()> {
        let subject = self.subject();
        let _lock = self.lock_tour_and_ref()?;
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

    /// A tour with no snapshot ref beside it was written after a drop or a move had deleted the review,
    /// as by a tour agent still writing then: it is no review, so it is deleted rather than shown.
    fn refuse_when_gone(&self) -> anyhow::Result<()> {
        let kept = self.work_tree.snapshot_refs()?;
        if kept.contains(&snapshot_ref(&self.change_set)) {
            return Ok(());
        }
        self.drop_review()?;
        anyhow::bail!(
            "the local review of {} was dropped or moved to a PR, so its tour is not kept; `guided-review open {}` starts a new one",
            self.subject(),
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
