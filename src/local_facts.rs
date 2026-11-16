use std::collections::BTreeSet;

use crate::file_path::FilePath;
use crate::pull_request::RepoName;
use crate::review_subject::{BranchName, WorkTreeRoot};

/// What the overview of a local review shows, computed from git each time and never stored.
#[derive(Debug, Clone, PartialEq)]
pub struct LocalFacts {
    pub repo: Option<RepoName>,
    pub folder: WorkTreeRoot,
    pub branch: BranchName,
    pub default_branch: String,
    /// The branch's commits that are not on the default branch, oldest first, as the branch reads.
    pub commit_subjects: Vec<String>,
    pub committed_files: BTreeSet<FilePath>,
    pub uncommitted_files: BTreeSet<FilePath>,
}

/// Where a file's change in a local review comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalChange {
    Committed,
    Uncommitted,
    Both,
}

impl LocalFacts {
    pub fn change_of(&self, path: &FilePath) -> Option<LocalChange> {
        match (
            self.committed_files.contains(path),
            self.uncommitted_files.contains(path),
        ) {
            (true, true) => Some(LocalChange::Both),
            (true, false) => Some(LocalChange::Committed),
            (false, true) => Some(LocalChange::Uncommitted),
            (false, false) => None,
        }
    }
}
