use std::fmt;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use anyhow::{Context, bail};
use sha2::{Digest, Sha256};

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

/// The root of a working tree, absolute and canonical, so `/tmp/x` and `/private/tmp/x` are one review. A
/// folder that is not there keeps the path it was given.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct WorkTreeRoot(PathBuf);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BranchName(String);

const LOCAL_KEY_PREFIX: &str = "local:";
/// The folder, under the tours folder and the state folder, that holds every local review's files.
pub const LOCAL_FILES_FOLDER: &str = "local";
/// Four bytes are the 8 hex characters of the short hash in a local review's folder name.
const SHORT_HASH_BYTES: usize = 4;

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

    /// What a question about the review as a whole is about, in the words the reviewer reads.
    pub const fn whole_change(&self) -> &'static str {
        match self {
            Self::PullRequest(_) => "the whole PR",
            Self::Local(_) => "the whole change",
        }
    }

    /// What the app answers an open request whose key is neither; the whole error chain is for logs.
    pub fn not_a_subject(key: &str) -> String {
        format!("`{key}` is not a PR link or a local key")
    }

    /// The name the app shows for it: `owner/repo#number` for a PR, `<folder name>@<branch>` for a
    /// local change set.
    pub fn short(&self) -> String {
        match self {
            Self::PullRequest(pr) => pr.short(),
            Self::Local(change_set) => format!(
                "{}@{}",
                change_set.folder.name(),
                change_set.branch.as_str()
            ),
        }
    }
}

/// Where the subject's files live, relative to the tours folder and the state folder: a folder, and a
/// file stem in it. A PR keeps `<owner>/<repo>/<number>`; a local change set is
/// `local/<folder name>-<short hash of the folder path>/<escaped branch>`.
impl ReviewSubject {
    pub fn files_folder(&self) -> PathBuf {
        match self {
            Self::PullRequest(pr) => PathBuf::from(pr.repo.to_string()),
            Self::Local(change_set) => change_set.files_folder(),
        }
    }

    pub fn file_stem(&self) -> String {
        match self {
            Self::PullRequest(pr) => pr.number.to_string(),
            Self::Local(change_set) => escape(change_set.branch.as_str(), &['%', '/']),
        }
    }
}

impl From<LocalChangeSet> for ReviewSubject {
    fn from(change_set: LocalChangeSet) -> Self {
        Self::Local(change_set)
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
    pub const fn new(folder: WorkTreeRoot, branch: BranchName, repo: Option<RepoName>) -> Self {
        Self {
            folder,
            branch,
            repo,
        }
    }

    /// The same change set with the repo its folder's clone belongs to; a key names no repo.
    #[must_use]
    pub fn in_repo(self, repo: Option<RepoName>) -> Self {
        Self { repo, ..self }
    }

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

    fn files_folder(&self) -> PathBuf {
        let path = self.folder.as_path();
        let name = self.folder.name();
        let digest = Sha256::digest(path.to_string_lossy().as_bytes());
        let hash = crate::hex::lower(digest.iter().take(SHORT_HASH_BYTES));
        PathBuf::from(LOCAL_FILES_FOLDER).join(format!("{name}-{hash}"))
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
        Ok(Self(std::fs::canonicalize(&folder).unwrap_or(folder)))
    }

    pub fn as_path(&self) -> &Path {
        &self.0
    }

    /// The folder's own name; `root` for `/`.
    pub fn name(&self) -> std::borrow::Cow<'_, str> {
        self.0
            .file_name()
            .map_or_else(|| "root".into(), |name| name.to_string_lossy())
    }
}

impl fmt::Display for BranchName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
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
