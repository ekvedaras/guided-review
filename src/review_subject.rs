use std::fmt;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use anyhow::{Context, bail};
use sha2::{Digest, Sha256};

use crate::pull_request::{HostSite, HostedRepo, PullRequestUrl, RepoName};

/// What a review reads: a PR, or a local change set.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ReviewSubject {
    PullRequest(PullRequestUrl),
    Local(LocalChangeSet),
}

/// One branch of one working tree. Its identity is the folder and the branch; `origin` is only for
/// display and for finding a PR opened from the branch, on the host the origin is on.
#[derive(Debug, Clone)]
pub struct LocalChangeSet {
    folder: WorkTreeRoot,
    branch: BranchName,
    origin: Option<HostedRepo>,
}

/// The root of a working tree, absolute and canonical, so `/tmp/x` and `/private/tmp/x` are one review. A
/// folder that is not there keeps the path it was given.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct WorkTreeRoot(PathBuf);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BranchName(String);

const LOCAL_KEY_PREFIX: &str = "local:";
const REMOTE_PREFIX: &str = "origin/";
/// A PR keeps the label it has always had; a local change set is labelled by its subject key.
const PR_LABEL_KEY: &str = "gr.pr";
const LOCAL_LABEL_KEY: &str = "gr.local";
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

    /// Whether the review has a PR page on GitHub to open.
    pub const fn has_host_page(&self) -> bool {
        matches!(self, Self::PullRequest(_))
    }

    /// Whether the review's threads live on GitHub, rather than in the review's state file.
    pub const fn has_host_threads(&self) -> bool {
        matches!(self, Self::PullRequest(_))
    }

    /// Whether a comment can join a pending review on GitHub, and be sent with a reviewer's note.
    pub const fn has_pending_review(&self) -> bool {
        matches!(self, Self::PullRequest(_))
    }

    /// A local review's comments are tasks for the agent that wrote the code, and it finishes by
    /// sending them.
    pub const fn comments_are_agent_tasks(&self) -> bool {
        matches!(self, Self::Local(_))
    }

    /// Whether the agent of the review changes the code in its folder, rather than only reading it.
    pub const fn agent_changes_code(&self) -> bool {
        matches!(self, Self::Local(_))
    }

    /// The repo whose code host holds the review; a local change set holds none.
    pub const fn host_repo(&self) -> Option<&RepoName> {
        match self {
            Self::PullRequest(pr) => Some(&pr.repo),
            Self::Local(_) => None,
        }
    }

    /// The key of a local change set, which names its files; `None` for a PR, whose files are named by
    /// its URL.
    pub fn local_key(&self) -> Option<String> {
        self.local().map(|_| self.to_string())
    }

    /// What the review's agent is called where the reviewer reads it.
    pub const fn agent_name(&self) -> &'static str {
        match self {
            Self::PullRequest(_) => "Tour agent",
            Self::Local(_) => "Agent",
        }
    }

    /// The herdr label key that claims the review's agent.
    pub const fn agent_label_key(&self) -> &'static str {
        match self {
            Self::PullRequest(_) => PR_LABEL_KEY,
            Self::Local(_) => LOCAL_LABEL_KEY,
        }
    }

    /// Every herdr label key an agent of a review can be claimed by.
    pub const AGENT_LABEL_KEYS: [&'static str; 2] = [PR_LABEL_KEY, LOCAL_LABEL_KEY];

    /// Whose changed files a basic tour holds, in the words the reviewer reads.
    pub const fn owner_of_changed_files(&self) -> &'static str {
        match self {
            Self::PullRequest(_) => "the PR's",
            Self::Local(_) => "the local change set's",
        }
    }

    /// What has happened when the review's code moved on from the head its tour is pinned to.
    pub const fn moved_on(&self) -> &'static str {
        match self {
            Self::PullRequest(_) => "PR has new commits",
            Self::Local(_) => "the review has a new snapshot of its folder",
        }
    }

    /// The folder the subject's code lives in, where its agent works: a local change set's own. A PR
    /// has none, so its agents work in a clone found for it.
    pub fn own_folder(&self) -> Option<&Path> {
        self.local().map(|change_set| change_set.folder().as_path())
    }

    /// What a question about the review as a whole is about, in the words the reviewer reads.
    pub const fn whole_change(&self) -> &'static str {
        match self {
            Self::PullRequest(_) => "the whole PR",
            Self::Local(_) => "the whole change",
        }
    }

    /// What the review is, in the words a prompt to an agent uses.
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::PullRequest(_) => "PR",
            Self::Local(_) => "local review",
        }
    }

    /// What names the review in a command, in the words a prompt to an agent uses.
    pub const fn key_name(&self) -> &'static str {
        match self {
            Self::PullRequest(_) => "PR url",
            Self::Local(_) => "local key",
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
            Self::PullRequest(pr) => pr.hosted_repo().files_folder(),
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
            None => key
                .parse()
                .map(Self::PullRequest)
                .with_context(|| {
                    format!(
                        "{key} is neither a PR link of a code host guided-review reads ({}) nor a local change set key",
                        HostSite::all_named()
                    )
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
    pub const fn new(folder: WorkTreeRoot, branch: BranchName, origin: Option<HostedRepo>) -> Self {
        Self {
            folder,
            branch,
            origin,
        }
    }

    /// The same change set with the repo its folder's clone belongs to; a key names no repo.
    #[must_use]
    pub fn with_origin(self, origin: Option<HostedRepo>) -> Self {
        Self { origin, ..self }
    }

    /// `<folder>@<branch>`: the folder's `@` is escaped, so the first `@` ends it.
    fn from_key(key: &str) -> anyhow::Result<Self> {
        let (folder, branch) = key
            .split_once('@')
            .context("it names no branch after an @")?;
        Ok(Self {
            folder: WorkTreeRoot::new(PathBuf::from(unescape(folder)?))?,
            branch: BranchName::new(branch)?,
            origin: None,
        })
    }

    fn files_folder(&self) -> PathBuf {
        PathBuf::from(LOCAL_FILES_FOLDER).join(self.folder.stem())
    }

    pub const fn folder(&self) -> &WorkTreeRoot {
        &self.folder
    }

    pub const fn branch(&self) -> &BranchName {
        &self.branch
    }

    /// The repo its folder's `origin` points at, and the host it is on.
    pub const fn origin(&self) -> Option<&HostedRepo> {
        self.origin.as_ref()
    }

    pub fn repo(&self) -> Option<&RepoName> {
        self.origin.as_ref().map(|origin| &origin.repo)
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

    /// `<folder name>-<short hash of the folder path>`.
    pub fn stem(&self) -> String {
        let digest = Sha256::digest(self.0.to_string_lossy().as_bytes());
        let hash = crate::hex::lower(digest.iter().take(SHORT_HASH_BYTES));
        format!("{}-{hash}", self.name())
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

    /// The branch itself, without the `origin/` of a remote-tracking ref: `origin/main` is `main`.
    pub fn without_remote(&self) -> Self {
        self.0
            .strip_prefix(REMOTE_PREFIX)
            .map_or_else(|| self.clone(), |name| Self(name.to_owned()))
    }
}
