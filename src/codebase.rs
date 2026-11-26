//! The codebase a reviewer's answer to "Use language servers?" is kept for.

use std::fmt;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use crate::pull_request::HostedRepo;
use crate::review_subject::{ReviewSubject, WorkTreeRoot};

/// What language servers are opted into and run on: a repo on a code host, or a local folder whose
/// origin is on no host the app reads. Written as the repo's text form (`owner/repo` on GitHub) or as
/// the folder's absolute path, which no repo can be mistaken for.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Codebase {
    Hosted(HostedRepo),
    Folder(WorkTreeRoot),
}

/// An owner no GitHub account can have, so a folder's state folder never meets a repo's.
const FOLDERS_OWNER: &str = "_folders";

impl Codebase {
    /// What the reviewer's yes or no to language servers is kept for, and whose server checkout
    /// the review reads from. A local change set's is the repo of its `origin`; a folder with none
    /// is its own codebase.
    pub fn of(subject: &ReviewSubject) -> Self {
        match subject {
            ReviewSubject::PullRequest(pr) => Self::Hosted(pr.hosted_repo()),
            ReviewSubject::Local(change_set) => change_set
                .origin()
                .cloned()
                .map_or_else(|| Self::Folder(change_set.folder().clone()), Self::Hosted),
        }
    }

    /// Where the app keeps this codebase's files under a state folder (server checkout, server
    /// index): the repo's files folder, or `_folders/<folder name>-<short hash of the path>`.
    pub fn state_folder(&self) -> PathBuf {
        match self {
            Self::Hosted(repo) => repo.files_folder(),
            Self::Folder(folder) => PathBuf::from(FOLDERS_OWNER).join(folder.stem()),
        }
    }
}

impl fmt::Display for Codebase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Hosted(repo) => repo.fmt(f),
            Self::Folder(folder) => folder.as_path().display().fmt(f),
        }
    }
}

impl FromStr for Codebase {
    type Err = anyhow::Error;

    fn from_str(text: &str) -> anyhow::Result<Self> {
        if Path::new(text).is_absolute() {
            return WorkTreeRoot::new(PathBuf::from(text)).map(Self::Folder);
        }
        text.parse().map(Self::Hosted)
    }
}
