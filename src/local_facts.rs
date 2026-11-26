use std::collections::BTreeSet;

use crate::file_path::FilePath;
use crate::pull_request::HostedRepo;
use crate::review_subject::{BranchName, WorkTreeRoot};

/// What the overview of a local review shows, computed from git each time and never stored.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LocalFacts {
    pub origin: Option<HostedRepo>,
    pub folder: WorkTreeRoot,
    pub branch: BranchName,
    pub default_branch: BranchName,
    /// The branch's commits that are not on the default branch, oldest first, as the branch reads.
    pub commit_subjects: Vec<String>,
    pub committed_files: BTreeSet<FilePath>,
    pub uncommitted_files: BTreeSet<FilePath>,
}

/// Where a file's change in a local review comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LocalChange {
    Committed,
    Uncommitted,
    Both,
}

impl LocalChange {
    /// What the overview and the file screen say of where the change comes from.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Committed => "committed",
            Self::Uncommitted => "uncommitted",
            Self::Both => "committed and uncommitted",
        }
    }
}

impl LocalFacts {
    /// "2 files uncommitted", or that there are none.
    pub fn uncommitted_label(&self) -> String {
        match self.uncommitted_files.len() {
            0 => "No uncommitted files".to_owned(),
            1 => "1 file uncommitted".to_owned(),
            count => format!("{count} files uncommitted"),
        }
    }

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
