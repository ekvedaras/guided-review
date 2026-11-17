use std::path::PathBuf;

use anyhow::Context;

use crate::app_folders::{AppFolders, remove_if_there};
use crate::local_work_tree::snapshot_ref;
use crate::review_subject::{LocalChangeSet, ReviewSubject};

/// The local reviews the app dropped or moved to a PR, one small file each, so a review whose snapshot
/// ref is missing can be told apart: with a record it went on purpose, without one the ref was lost.
/// Starting the review again forgets its record.
pub struct GoneLocalReviews {
    folder: PathBuf,
}

impl GoneLocalReviews {
    pub fn in_folders(folders: &AppFolders) -> Self {
        Self {
            folder: folders.gone_local_reviews(),
        }
    }

    pub fn record(&self, change_set: &LocalChangeSet) -> anyhow::Result<()> {
        std::fs::create_dir_all(&self.folder)
            .with_context(|| format!("could not create {}", self.folder.display()))?;
        let file = self.file_of(change_set);
        std::fs::write(&file, ReviewSubject::from(change_set.clone()).to_string())
            .with_context(|| format!("could not write {}", file.display()))
    }

    pub fn is_recorded(&self, change_set: &LocalChangeSet) -> anyhow::Result<bool> {
        let file = self.file_of(change_set);
        file.try_exists()
            .with_context(|| format!("could not look for {}", file.display()))
    }

    pub fn forget(&self, change_set: &LocalChangeSet) -> anyhow::Result<()> {
        let file = self.file_of(change_set);
        remove_if_there(&file, std::fs::remove_file(&file))
    }

    fn file_of(&self, change_set: &LocalChangeSet) -> PathBuf {
        let reference = snapshot_ref(change_set);
        let name = reference.rsplit('/').next().unwrap_or(&reference);
        self.folder.join(name)
    }
}
