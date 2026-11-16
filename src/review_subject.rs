use std::fmt;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use anyhow::{Context, bail};

use crate::pull_request::{PullRequestUrl, RepoName};

/// What a review reads: a PR, or a local change set.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ReviewSubject {
    PullRequest(PullRequestUrl),
    Local(LocalChangeSet),
}

/// One branch of one working tree. Its identity is the folder and the branch; `repo` is only for
/// display and for finding a PR opened from the branch.
#[derive(Debug, Clone)]
pub struct LocalChangeSet {
    folder: WorkTreeRoot,
    branch: BranchName,
    repo: Option<RepoName>,
}

/// The root of a working tree, absolute.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct WorkTreeRoot(PathBuf);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BranchName(String);

const LOCAL_KEY_PREFIX: &str = "local:";

impl ReviewSubject {
    pub const fn pull_request(&self) -> Option<&PullRequestUrl> {
        match self {
            Self::PullRequest(pr) => Some(pr),
            Self::Local(_) => None,
        }
    }

    pub const fn local(&self) -> Option<&LocalChangeSet> {
        match self {
            Self::PullRequest(_) => None,
            Self::Local(change_set) => Some(change_set),
        }
    }
}

impl From<PullRequestUrl> for ReviewSubject {
    fn from(pr: PullRequestUrl) -> Self {
        Self::PullRequest(pr)
    }
}

/// The subject key: a PR is its URL, a local change set is `local:<folder>@<branch>`, with `%` and `@`
/// in the folder escaped so a branch may hold an `@`.
impl FromStr for ReviewSubject {
    type Err = anyhow::Error;

    fn from_str(key: &str) -> anyhow::Result<Self> {
        match key.strip_prefix(LOCAL_KEY_PREFIX) {
            Some(local) => LocalChangeSet::from_key(local)
                .map(Self::Local)
                .with_context(|| format!("not a local change set key: {key}")),
            None => key.parse().map(Self::PullRequest).with_context(|| {
                format!("{key} is neither a GitHub PR link nor a local change set key")
            }),
        }
    }
}

impl fmt::Display for ReviewSubject {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PullRequest(pr) => pr.fmt(f),
            Self::Local(change_set) => write!(
                f,
                "{LOCAL_KEY_PREFIX}{}@{}",
                escape(&change_set.folder.0.to_string_lossy(), &['%', '@']),
                change_set.branch.0
            ),
        }
    }
}

/// The characters a key part escapes, with their escapes.
const ESCAPES: [(char, &str); 3] = [('%', "%25"), ('@', "%40"), ('/', "%2F")];

fn escape(text: &str, special: &[char]) -> String {
    text.chars().fold(String::new(), |mut escaped, character| {
        match ESCAPES
            .iter()
            .find(|(plain, _)| *plain == character && special.contains(plain))
        {
            Some((_, escape)) => escaped.push_str(escape),
            None => escaped.push(character),
        }
        escaped
    })
}

fn unescape(text: &str) -> anyhow::Result<String> {
    let mut plain = String::new();
    let mut rest = text;
    while let Some((before, after)) = rest.split_once('%') {
        plain.push_str(before);
        let (character, after_escape) = ESCAPES
            .iter()
            .find_map(|(character, escape)| {
                let code = escape.strip_prefix('%')?;
                Some((*character, after.strip_prefix(code)?))
            })
            .with_context(|| format!("an unknown escape in {text}"))?;
        plain.push(character);
        rest = after_escape;
    }
    plain.push_str(rest);
    Ok(plain)
}

impl LocalChangeSet {
    /// `<folder>@<branch>`: the folder's `@` is escaped, so the first `@` ends it.
    fn from_key(key: &str) -> anyhow::Result<Self> {
        let (folder, branch) = key
            .split_once('@')
            .context("it names no branch after an @")?;
        Ok(Self {
            folder: WorkTreeRoot::new(PathBuf::from(unescape(folder)?))?,
            branch: BranchName::new(branch)?,
            repo: None,
        })
    }

    pub const fn folder(&self) -> &WorkTreeRoot {
        &self.folder
    }

    pub const fn branch(&self) -> &BranchName {
        &self.branch
    }

    pub const fn repo(&self) -> Option<&RepoName> {
        self.repo.as_ref()
    }
}

impl PartialEq for LocalChangeSet {
    fn eq(&self, other: &Self) -> bool {
        self.folder == other.folder && self.branch == other.branch
    }
}

impl Eq for LocalChangeSet {}

impl std::hash::Hash for LocalChangeSet {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.folder.hash(state);
        self.branch.hash(state);
    }
}

impl WorkTreeRoot {
    pub fn new(folder: PathBuf) -> anyhow::Result<Self> {
        if !folder.is_absolute() {
            bail!("the folder {} is not an absolute path", folder.display());
        }
        Ok(Self(folder))
    }

    pub fn as_path(&self) -> &Path {
        &self.0
    }
}

impl BranchName {
    pub fn new(name: &str) -> anyhow::Result<Self> {
        if name.trim().is_empty() {
            bail!("the branch name is empty");
        }
        Ok(Self(name.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}
