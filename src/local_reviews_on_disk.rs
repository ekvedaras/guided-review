//! The local reviews whose tours are on disk, the tours that could not be read, and the clean-up of
//! the snapshot refs no review on disk keeps.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::app_folders::AppFolders;
use crate::local_work_tree::{LocalWorkTree, SharedGitFolder, snapshot_ref};
use crate::review_subject::{LOCAL_FILES_FOLDER, LocalChangeSet, ReviewSubject};
use crate::tour::Tour;

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
