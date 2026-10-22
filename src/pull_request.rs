use std::fmt;
use std::str::FromStr;

use anyhow::{Context, bail};

#[derive(Debug, Clone)]
pub struct RepoName {
    owner: String,
    name: String,
}

impl RepoName {
    pub fn owner(&self) -> &str {
        &self.owner
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn new(owner: &str, name: &str) -> Self {
        Self {
            owner: owner.to_string(),
            name: name.trim_end_matches(".git").to_string(),
        }
    }

    pub fn from_github_remote(url: &str) -> Option<Self> {
        let after_host = url
            .strip_prefix("git@github.com:")
            .or_else(|| url.split_once("github.com/").map(|(_, rest)| rest))?;
        let mut segments = after_host.trim_end_matches('/').split('/');
        let (owner, name) = (segments.next()?, segments.next()?);
        if segments.next().is_some() || owner.is_empty() || name.is_empty() {
            return None;
        }
        Some(Self::new(owner, name))
    }
}

impl PartialEq for RepoName {
    fn eq(&self, other: &Self) -> bool {
        self.owner.eq_ignore_ascii_case(&other.owner) && self.name.eq_ignore_ascii_case(&other.name)
    }
}

impl fmt::Display for RepoName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.owner, self.name)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PullRequestUrl {
    pub repo: RepoName,
    pub number: u64,
}

impl FromStr for PullRequestUrl {
    type Err = anyhow::Error;

    fn from_str(url: &str) -> anyhow::Result<Self> {
        let path = url
            .strip_prefix("https://github.com/")
            .with_context(|| format!("not a GitHub PR link: {url}"))?;
        let segments: Vec<&str> = path.split('/').collect();
        let [owner, name, "pull", number, ..] = segments.as_slice() else {
            bail!("not a GitHub PR link: {url}");
        };
        let number = number
            .parse()
            .with_context(|| format!("not a GitHub PR link: {url}"))?;
        Ok(Self {
            repo: RepoName::new(owner, name),
            number,
        })
    }
}

impl PullRequestUrl {
    /// `owner/repo#number`
    pub fn short(&self) -> String {
        format!("{}#{}", self.repo, self.number)
    }
}

impl fmt::Display for PullRequestUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "https://github.com/{}/pull/{}", self.repo, self.number)
    }
}

/// Empty when not known yet: a tour that was never pinned, a state file from before shas were kept.
#[derive(
    Debug,
    Clone,
    Default,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    serde::Serialize,
    serde::Deserialize,
)]
#[serde(transparent)]
pub struct Sha(String);

impl Sha {
    pub fn new(sha: &str) -> Self {
        Self(sha.trim().to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Display for Sha {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

pub struct PullRequestTips {
    pub base_branch_tip: Sha,
    pub head: Sha,
}
