//! The codebase a reviewer's answer to "Use language servers?" is kept for.

use std::fmt;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use crate::pull_request::RepoName;
use crate::review_subject::{ReviewSubject, WorkTreeRoot};

/// What language servers are opted into and run on: a GitHub repo, or a local folder that has no
/// GitHub origin. Written as `owner/repo` or as the folder's absolute path, which no `owner/repo`
/// can be mistaken for.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Codebase {
    GitHub(RepoName),
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
            ReviewSubject::PullRequest(pr) => Self::GitHub(pr.repo.clone()),
            ReviewSubject::Local(change_set) => change_set
                .repo()
                .cloned()
                .map_or_else(|| Self::Folder(change_set.folder().clone()), Self::GitHub),
        }
    }

    /// Where the app keeps this codebase's files under a state folder (server checkout, server
    /// index): `owner/repo`, or `_folders/<folder name>-<short hash of the path>`.
    pub fn state_folder(&self) -> PathBuf {
        match self {
            Self::GitHub(repo) => PathBuf::from(repo.owner()).join(repo.name()),
            Self::Folder(folder) => PathBuf::from(FOLDERS_OWNER).join(folder.stem()),
        }
    }
}

impl fmt::Display for Codebase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::GitHub(repo) => repo.fmt(f),
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
        text.parse().map(Self::GitHub)
    }
}
