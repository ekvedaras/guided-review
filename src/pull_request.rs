use std::fmt;
use std::path::{Component, Path, PathBuf};
use std::str::FromStr;

use anyhow::bail;

/// A repo's path on its host: the namespace it is in and its own name. GitHub's namespace is the owner;
/// GitLab's is a group with any number of subgroups (`group/sub/project`).
#[derive(Debug, Clone)]
pub struct RepoName {
    namespace: String,
    name: String,
}

impl RepoName {
    /// GitHub's owner, GitLab's group path.
    pub fn namespace(&self) -> &str {
        &self.namespace
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn new(namespace: &str, name: &str) -> Self {
        Self {
            namespace: namespace.to_owned(),
            name: name.trim_end_matches(".git").to_owned(),
        }
    }
}

impl PartialEq for RepoName {
    fn eq(&self, other: &Self) -> bool {
        self.namespace.eq_ignore_ascii_case(&other.namespace)
            && self.name.eq_ignore_ascii_case(&other.name)
    }
}

impl Eq for RepoName {}

impl std::hash::Hash for RepoName {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.namespace.to_ascii_lowercase().hash(state);
        self.name.to_ascii_lowercase().hash(state);
    }
}

impl fmt::Display for RepoName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.namespace, self.name)
    }
}

/// `namespace/name`, as it is shown: two segments or more, none of them empty.
impl FromStr for RepoName {
    type Err = anyhow::Error;

    fn from_str(full_name: &str) -> anyhow::Result<Self> {
        match full_name.rsplit_once('/') {
            Some((namespace, name))
                if !name.is_empty() && namespace.split('/').all(|segment| !segment.is_empty()) =>
            {
                Ok(Self::new(namespace, name))
            }
            _ => bail!("not a namespace/name repo: {full_name}"),
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

    /// The host's key where a text form names it: `github`.
    pub const fn key(self) -> &'static str {
        match self {
            Self::GitHub => "github",
        }
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

/// `<host key>:namespace/name`. A bare `namespace/name` is GitHub's, the one host the app read
/// before hosts were named, so every key written then (`config.toml`'s `[clones]`) reads the same.
impl FromStr for HostedRepo {
    type Err = anyhow::Error;

    fn from_str(text: &str) -> anyhow::Result<Self> {
        let named = HostSite::ALL.into_iter().find_map(|host| {
            let repo = text.strip_prefix(host.key())?.strip_prefix(':')?;
            Some((host, repo))
        });
        let (host, repo) = named.unwrap_or((HostSite::GitHub, text));
        Ok(Self {
            host,
            repo: repo.parse()?,
        })
    }
}

impl HostedRepo {
    /// Where the app keeps the repo's files under a folder it keys by repo (state, tours, server
    /// checkouts, server indexes): GitHub's at `<owner>/<repo>`, as before hosts were named. A second
    /// host's go under `_<host key>/`, which no GitHub owner can be, so the two never meet.
    pub fn files_folder(&self) -> PathBuf {
        match self.host {
            HostSite::GitHub => PathBuf::from(self.repo.namespace()).join(self.repo.name()),
        }
    }

    /// The repo whose files folder `folder` is, relative to the folder it is under.
    pub fn of_files_folder(folder: &Path) -> Option<Self> {
        let segments: Vec<&str> = folder
            .components()
            .map(|component| match component {
                Component::Normal(segment) => segment.to_str(),
                _ => None,
            })
            .collect::<Option<_>>()?;
        HostSite::ALL.into_iter().find_map(|host| match host {
            HostSite::GitHub => match segments.as_slice() {
                [owner, name] if !owner.starts_with('_') => Some(Self {
                    host,
                    repo: RepoName::new(owner, name),
                }),
                _ => None,
            },
        })
    }
}

/// GitHub's repo is written bare, as it was before hosts were named.
impl fmt::Display for HostedRepo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.host {
            HostSite::GitHub => self.repo.fmt(f),
        }
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
