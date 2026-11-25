use std::fmt;
use std::str::FromStr;

use anyhow::bail;

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
            owner: owner.to_owned(),
            name: name.trim_end_matches(".git").to_owned(),
        }
    }
}

impl PartialEq for RepoName {
    fn eq(&self, other: &Self) -> bool {
        self.owner.eq_ignore_ascii_case(&other.owner) && self.name.eq_ignore_ascii_case(&other.name)
    }
}

impl Eq for RepoName {}

impl std::hash::Hash for RepoName {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.owner.to_ascii_lowercase().hash(state);
        self.name.to_ascii_lowercase().hash(state);
    }
}

impl fmt::Display for RepoName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.owner, self.name)
    }
}

/// `owner/name`, as it is shown.
impl FromStr for RepoName {
    type Err = anyhow::Error;

    fn from_str(full_name: &str) -> anyhow::Result<Self> {
        match full_name.split_once('/') {
            Some((owner, name)) if !owner.is_empty() && !name.is_empty() && !name.contains('/') => {
                Ok(Self::new(owner, name))
            }
            _ => bail!("not an owner/name repo: {full_name}"),
        }
    }
}

impl serde::Serialize for RepoName {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> serde::Deserialize<'de> for RepoName {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let full_name = String::deserialize(deserializer)?;
        full_name.parse().map_err(serde::de::Error::custom)
    }
}

/// The code host a PR lives on, as its link names it. What the host shows on the web, and how its
/// links read, is `host_web`'s.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HostSite {
    GitHub,
}

impl HostSite {
    pub const ALL: [Self; 1] = [Self::GitHub];

    /// The hosts the app reads, by name, for a message that refuses another.
    pub fn all_named() -> String {
        Self::ALL.map(Self::name).join(", ")
    }

    /// The host's own name, which the UI shows.
    pub const fn name(self) -> &'static str {
        match self {
            Self::GitHub => "GitHub",
        }
    }
}

/// A PR: its host, its repo there and its number. Its text form, the subject key, is its page on
/// the host (`host_web`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PullRequestUrl {
    pub host: HostSite,
    pub repo: RepoName,
    pub number: u64,
}

impl PullRequestUrl {
    /// `owner/repo#number`
    pub fn short(&self) -> String {
        format!("{}#{}", self.repo, self.number)
    }

    pub fn hosted_repo(&self) -> HostedRepo {
        HostedRepo {
            host: self.host,
            repo: self.repo.clone(),
        }
    }
}

/// A repo on a code host: which host, and the repo's name there. A git remote names both
/// (`host_web`), so a local change set's origin knows which host to ask for its branch's PR.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct HostedRepo {
    pub host: HostSite,
    pub repo: RepoName,
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
        Self(sha.trim().to_owned())
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

/// The two tips a change is read between: where its base branch is, and its head.
pub struct ChangeTips {
    pub base_branch_tip: Sha,
    pub head: Sha,
}
