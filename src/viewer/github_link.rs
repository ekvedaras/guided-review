use std::fmt;

use crate::code_source::FileStatus;
use crate::github::percent_encode_path;
use crate::pieces::ChangedFile;
use crate::pull_request::{PullRequestUrl, Sha};

use super::step_page::{LineLocation, Side};

/// A page on github.com. Made from the tour's `pr` and commits; the tour file never holds links.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitHubLink(String);

impl GitHubLink {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for GitHubLink {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Links point at the commits the tour is pinned to, so a line stays the same line
/// even after the PR has moved on.
pub(super) struct GitHubLinks<'a> {
    pub pr: &'a PullRequestUrl,
    pub base: &'a Sha,
    pub head: &'a Sha,
}

impl GitHubLinks<'_> {
    pub fn to_line(&self, file: &ChangedFile, location: &LineLocation) -> GitHubLink {
        let at = match location.side {
            Side::Base => self.blob(self.base, file.old_path.as_deref().unwrap_or(&file.path)),
            Side::Head => self.blob(self.head, &file.path),
        };
        GitHubLink(format!("{at}#L{}", location.line))
    }

    /// A deleted file only exists at base.
    pub fn to_file(&self, file: &ChangedFile) -> GitHubLink {
        GitHubLink(match file.status {
            FileStatus::Deleted => self.blob(self.base, &file.path),
            _ => self.blob(self.head, &file.path),
        })
    }

    fn blob(&self, sha: &Sha, path: &str) -> String {
        format!(
            "https://github.com/{}/blob/{sha}/{}",
            self.pr.repo,
            percent_encode_path(path)
        )
    }
}
