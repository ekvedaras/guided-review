//! A PR's code host on the web: the PR's page, which is its text form, its checks, a file at a
//! commit, and how a link or a git remote of the host reads. Each host's adapter knows its own
//! shapes (`github_web`); this picks the one of the PR's host, or the host a link is on.

use std::fmt;
use std::str::FromStr;

use anyhow::bail;

use crate::file_path::FilePath;
use crate::github_web;
use crate::line_number::LineNumber;
use crate::pull_request::{HostSite, HostedRepo, PullRequestUrl, RepoName, Sha};

impl HostSite {
    fn of_link(url: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|site| match site {
            Self::GitHub => github_web::is_link(url),
        })
    }

    pub fn file_page(
        self,
        repo: &RepoName,
        sha: &Sha,
        path: &FilePath,
        line: Option<LineNumber>,
    ) -> String {
        match self {
            Self::GitHub => github_web::file_page(repo, sha, path, line),
        }
    }

    /// The file and line of `repo` a link to a file on this host points at.
    pub fn file_of_link(self, repo: &RepoName, url: &str) -> Option<(FilePath, LineNumber)> {
        match self {
            Self::GitHub => github_web::file_of_link(repo, url),
        }
    }
}

impl PullRequestUrl {
    /// How the host writes a reference to the PR in text: GitHub's `owner/repo#7` (GitLab would
    /// write `group/project!7`).
    pub fn short(&self) -> String {
        match self.host {
            HostSite::GitHub => github_web::pull_request_reference(&self.repo, self.number),
        }
    }

    pub fn checks_page(&self) -> String {
        match self.host {
            HostSite::GitHub => github_web::checks_page(&self.repo, self.number),
        }
    }

    /// The ref of the host's repo that holds the PR's head, to fetch it by.
    pub fn head_ref(&self) -> String {
        match self.host {
            HostSite::GitHub => github_web::head_ref(self.number),
        }
    }
}

/// A PR's page on any code host the app reads.
impl FromStr for PullRequestUrl {
    type Err = anyhow::Error;

    fn from_str(url: &str) -> anyhow::Result<Self> {
        match HostSite::of_link(url) {
            Some(host @ HostSite::GitHub) => {
                let (repo, number) = github_web::pull_request_of(url)?;
                Ok(Self { host, repo, number })
            }
            None => bail!(
                "not a PR link of a code host guided-review reads ({}): {url}",
                HostSite::all_named()
            ),
        }
    }
}

impl fmt::Display for PullRequestUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.host {
            HostSite::GitHub => {
                f.write_str(&github_web::pull_request_page(&self.repo, self.number))
            }
        }
    }
}

impl HostedRepo {
    /// The repo a git remote points at, and the host it is on; `None` for a host the app does not read.
    pub fn of_remote(url: &str) -> Option<Self> {
        HostSite::ALL.into_iter().find_map(|host| {
            let repo = match host {
                HostSite::GitHub => github_web::repo_of_remote(url),
            }?;
            Some(Self { host, repo })
        })
    }
}
