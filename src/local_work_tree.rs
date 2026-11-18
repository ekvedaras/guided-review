use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::fmt::Display;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use sha2::{Digest, Sha256};

use crate::file_path::FilePath;
use crate::local_facts::LocalFacts;
use crate::local_git::LocalGit;
use crate::pull_request::{ChangeTips, PullRequestUrl, Sha};
use crate::review_subject::{BranchName, LocalChangeSet, ReviewSubject, WorkTreeRoot};

/// Why `open` stops before it makes a review.
#[derive(Debug, thiserror::Error)]
pub enum OpenRefusal {
    #[error("{} is not inside a git clone", .0.display())]
    NotAClone(PathBuf),
    #[error("{} is on a detached HEAD, not on a branch; check out a branch to review it", .0.display())]
    DetachedHead(PathBuf),
    #[error(
        "{} has no default branch to compare with: looked for refs/remotes/origin/HEAD, main and master",
        .0.display()
    )]
    NoDefaultBranch(PathBuf),
    #[error(
        "nothing to review on {branch} in {}: no commits beyond {default_branch} and no uncommitted files",
        .folder.display()
    )]
    NothingToReview {
        folder: PathBuf,
        branch: BranchName,
        default_branch: BranchName,
    },
    #[error("git failed in {}: {reason}", .folder.display())]
    GitFailed { folder: PathBuf, reason: String },
}

/// The branch a change set is compared with, and where it is.
#[derive(Debug, Clone)]
pub struct DefaultBranch {
    pub name: BranchName,
    pub tip: Sha,
}

/// The commit a local review reads: a commit of the working tree, or `HEAD` itself when it is clean.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    pub head: Sha,
}

/// What a folder's branch has to review, before any review of it exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Glance {
    pub commits_ahead: usize,
    pub uncommitted_files: usize,
}

impl Glance {
    pub const fn is_nothing_to_review(self) -> bool {
        self.commits_ahead == 0 && self.uncommitted_files == 0
    }
}

/// What the folder of a local review is now, against the snapshot the review reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OnDisk {
    /// On the review's branch, at the `HEAD` the snapshot was made over, with the snapshot's files.
    Same,
    /// On the review's branch, but it got commits or its files changed: a new snapshot would differ.
    Changed,
    /// On another branch, or on no branch (`None`) after a checkout of a commit.
    OnAnotherBranch(Option<BranchName>),
    /// The review's branch, or its whole folder, is no more.
    BranchGone,
}

/// The git adapter of a local review: the one place that runs git in the reviewer's own working tree.
/// It never changes their index, working tree, branches or stash; its only write is the snapshot's ref
/// and the commit object behind it.
#[derive(Debug, Clone)]
pub struct LocalWorkTree {
    root: WorkTreeRoot,
}

/// The `.git` folder of a repository's main working tree, canonical, which all its linked worktrees
/// share: one per repository, however many worktrees it has.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SharedGitFolder(PathBuf);

const SNAPSHOT_REF_PREFIX: &str = "refs/guided-review/local-";
/// Six bytes are the 12 hex characters that tell one local review's ref from another's.
const SNAPSHOT_REF_HASH_BYTES: usize = 6;
/// Refs that keep the snapshots a PR's review still reads a moved file or step at; never matched by
/// `SNAPSHOT_REF_PREFIX`, so the clean-up of local reviews leaves them be.
const READ_HEAD_REF_PREFIX: &str = "refs/guided-review/read-";
const SNAPSHOT_IDENTITY_NAME: &str = "guided-review";
const SNAPSHOT_IDENTITY_EMAIL: &str = "snapshot@guided-review.invalid";

impl LocalWorkTree {
    /// The working tree any folder in a clone, or in a linked git worktree, belongs to.
    pub fn containing(path: &Path) -> Result<Self, OpenRefusal> {
        let output = Command::new("git")
            .arg("-C")
            .arg(path)
            .args(["rev-parse", "--show-toplevel"])
            .output()
            .map_err(|error| failed(path, error))?;
        if !output.status.success() {
            return Err(OpenRefusal::NotAClone(path.to_path_buf()));
        }
        let top_level = String::from_utf8_lossy(&output.stdout);
        // The temp folder of a Mac is a symlink; a key must name the folder one way only.
        let canonical = Path::new(top_level.trim_end())
            .canonicalize()
            .map_err(|error| failed(path, error))?;
        let root = WorkTreeRoot::new(canonical).map_err(|error| failed(path, error))?;
        Ok(Self { root })
    }

    /// For a root already known, such as the folder of a local review's key.
    pub const fn at(root: WorkTreeRoot) -> Self {
        Self { root }
    }

    pub const fn root(&self) -> &WorkTreeRoot {
        &self.root
    }

    pub fn change_set(&self) -> Result<LocalChangeSet, OpenRefusal> {
        // `symbolic-ref` fails exactly when HEAD is not a branch.
        let branch = self
            .git(&["symbolic-ref", "--quiet", "--short", "HEAD"])
            .map_err(|_not_a_branch| OpenRefusal::DetachedHead(self.folder()))?;
        let branch = BranchName::new(&branch).map_err(|error| self.failed(error))?;
        Ok(LocalChangeSet::new(
            self.root.clone(),
            branch,
            LocalGit::origin_of(self.root.as_path()),
        ))
    }

    /// `refs/remotes/origin/HEAD` as the clone knows it, else `main`, else `master`. Nothing is fetched.
    pub fn default_branch(&self) -> Result<DefaultBranch, OpenRefusal> {
        let remote_head = self
            .git(&["symbolic-ref", "--quiet", "refs/remotes/origin/HEAD"])
            .ok()
            .and_then(|full| {
                let name = full.strip_prefix("refs/remotes/")?.to_owned();
                Some((name, full))
            });
        let local = ["main", "master"].map(|name| (name.to_owned(), format!("refs/heads/{name}")));
        for (name, full) in remote_head.into_iter().chain(local) {
            if let Ok(tip) = self.git(&[
                "rev-parse",
                "--verify",
                "--quiet",
                &format!("{full}^{{commit}}"),
            ]) {
                return Ok(DefaultBranch {
                    name: BranchName::new(&name).map_err(|error| self.failed(error))?,
                    tip: Sha::new(&tip),
                });
            }
        }
        Err(OpenRefusal::NoDefaultBranch(self.folder()))
    }

    /// Nothing to review: no commits beyond the merge base with the default branch, and a working tree
    /// that is `HEAD`'s. Checked without making a snapshot, so a refusal leaves no ref behind.
    pub fn refuse_when_nothing_to_review(
        &self,
        change_set: &LocalChangeSet,
    ) -> Result<(), OpenRefusal> {
        let default = self.default_branch()?;
        let head = self.head()?;
        let base = self.git(&["merge-base", default.tip.as_str(), head.as_str()])?;
        let commits_ahead = self.git(&["rev-list", "--count", &format!("{base}..{head}")])?;
        if commits_ahead != "0" || self.working_tree_id()? != self.tree_of("HEAD")? {
            return Ok(());
        }
        Err(OpenRefusal::NothingToReview {
            folder: self.folder(),
            branch: change_set.branch().clone(),
            default_branch: default.name,
        })
    }

    /// How much there is to review in the folder now, for a list of folders: no snapshot is made and no ref
    /// is written, as with `refuse_when_nothing_to_review`.
    pub fn glance(&self) -> Result<Glance, OpenRefusal> {
        let default = self.default_branch()?;
        let head = self.head()?;
        let base = self.git(&["merge-base", default.tip.as_str(), head.as_str()])?;
        let commits_ahead = self
            .git(&["rev-list", "--count", &format!("{base}..{head}")])?
            .parse()
            .map_err(|error| self.failed(error))?;
        let uncommitted_files = self.uncommitted_file_count()?;
        Ok(Glance {
            commits_ahead,
            uncommitted_files,
        })
    }

    /// The commit of the working tree, kept by no ref yet. It has `HEAD` as its parent, from a temporary
    /// index; with a clean working tree it is `HEAD` itself and nothing is committed. It is dated as
    /// `HEAD` is, so the same files over the same `HEAD` are always the same commit, whoever makes it.
    pub fn make_snapshot(&self) -> Result<Snapshot, OpenRefusal> {
        let head = self.head()?;
        let tree = self.working_tree_id()?;
        if tree == self.tree_of("HEAD")? {
            return Ok(Snapshot { head });
        }
        let date = self.git(&["log", "-1", "--format=%cd", "--date=raw", head.as_str()])?;
        let identity = [
            ("GIT_AUTHOR_NAME", SNAPSHOT_IDENTITY_NAME),
            ("GIT_AUTHOR_EMAIL", SNAPSHOT_IDENTITY_EMAIL),
            ("GIT_AUTHOR_DATE", &date),
            ("GIT_COMMITTER_NAME", SNAPSHOT_IDENTITY_NAME),
            ("GIT_COMMITTER_EMAIL", SNAPSHOT_IDENTITY_EMAIL),
            ("GIT_COMMITTER_DATE", &date),
        ]
        .map(|(name, value)| (name, OsStr::new(value)));
        let commit = self.git_with(
            &identity,
            &[
                "-c",
                "commit.gpgsign=false",
                "commit-tree",
                &tree,
                "-p",
                head.as_str(),
                "-m",
                "guided-review snapshot of the working tree",
            ],
        )?;
        Ok(Snapshot {
            head: Sha::new(&commit),
        })
    }

    /// Deletes the change set's ref, when there is one: `git gc` may take its snapshot then. Only the
    /// local adapter of `opened_subject` calls it, under the tour-and-ref lock.
    pub(crate) fn forget_snapshot(&self, change_set: &LocalChangeSet) -> Result<(), OpenRefusal> {
        self.delete_snapshot_ref(&snapshot_ref(change_set))
    }

    /// The git folder this working tree shares with every linked worktree of its repository, and with
    /// it the refs: a snapshot ref written from one worktree is in all of them.
    pub fn shared_git_folder(&self) -> Result<SharedGitFolder, OpenRefusal> {
        let named = self.git(&["rev-parse", "--path-format=absolute", "--git-common-dir"])?;
        let canonical = Path::new(&named)
            .canonicalize()
            .map_err(|error| self.failed(error))?;
        Ok(SharedGitFolder(canonical))
    }

    /// The refs that keep snapshots in this working tree's repository, of any local review in any of
    /// its linked worktrees; never a PR's fetch ref.
    pub fn snapshot_refs(&self) -> Result<Vec<String>, OpenRefusal> {
        Ok(self
            .git(&[
                "for-each-ref",
                "--format=%(refname)",
                &format!("{SNAPSHOT_REF_PREFIX}*"),
            ])?
            .lines()
            .map(str::to_owned)
            .collect())
    }

    /// Deletes one of `snapshot_refs`; any other ref is refused. A ref that is not there is already
    /// gone, as git says; a folder no longer on disk has no git left to delete it from.
    pub fn delete_snapshot_ref(&self, name: &str) -> Result<(), OpenRefusal> {
        if !name.starts_with(SNAPSHOT_REF_PREFIX) {
            return Err(self.failed(format!("{name} is not a snapshot ref")));
        }
        if !self.root.as_path().exists() {
            return Ok(());
        }
        self.git(&["update-ref", "-d", name]).map(drop)
    }

    /// Points the change set's one ref at `snapshot`, so `git gc` keeps it. Only the local adapter of
    /// `opened_subject` calls it, under the tour-and-ref lock, so the ref never moves off the snapshot
    /// the tour on disk reads.
    pub(crate) fn keep_snapshot(
        &self,
        change_set: &LocalChangeSet,
        snapshot: &Snapshot,
    ) -> Result<(), OpenRefusal> {
        self.git(&[
            "update-ref",
            &snapshot_ref(change_set),
            snapshot.head.as_str(),
        ])
        .map(drop)
    }

    /// Keeps each of `heads` from `git gc` with a ref of the PR's own, as a moved review reads files at
    /// snapshots no local review keeps once it is gone.
    pub fn keep_read_heads(
        &self,
        pr: &PullRequestUrl,
        heads: &BTreeSet<Sha>,
    ) -> Result<(), OpenRefusal> {
        for head in heads {
            self.git(&["update-ref", &read_head_ref(pr, head), head.as_str()])?;
        }
        Ok(())
    }

    /// Deletes the PR's read-head refs of heads it no longer reads at. Returns how many it keeps.
    pub fn let_go_of_read_heads(
        &self,
        pr: &PullRequestUrl,
        still_read_at: &BTreeSet<Sha>,
    ) -> Result<usize, OpenRefusal> {
        if !self.root.as_path().exists() {
            return Ok(0);
        }
        let kept: BTreeSet<String> = still_read_at
            .iter()
            .map(|head| read_head_ref(pr, head))
            .collect();
        let of_the_pr = self.git(&[
            "for-each-ref",
            "--format=%(refname)",
            &format!("{}*", read_head_ref_prefix_of(pr)),
        ])?;
        let mut still_kept = 0;
        for name in of_the_pr.lines() {
            if kept.contains(name) {
                still_kept += 1;
            } else {
                self.git(&["update-ref", "-d", name])?;
            }
        }
        Ok(still_kept)
    }

    /// `(default branch tip, snapshot)`, the tips a PR has too.
    pub fn tips(&self, snapshot: &Snapshot) -> Result<ChangeTips, OpenRefusal> {
        Ok(ChangeTips {
            base_branch_tip: self.default_branch()?.tip,
            head: snapshot.head.clone(),
        })
    }

    /// Whether the files on disk, untracked ones included and ignored ones left out, are the snapshot's
    /// tree. `false` when git cannot tell, as for any `CodeSource` that cannot open a file in an IDE.
    pub fn working_tree_matches(&self, snapshot: &Sha) -> bool {
        self.files_match(snapshot).unwrap_or(false)
    }

    /// Compares the files on disk with an index of the snapshot's own, so nothing is written to the
    /// clone's objects, as making a tree of the files would.
    fn files_match(&self, snapshot: &Sha) -> Result<bool, OpenRefusal> {
        let index = TemporaryIndex::new();
        let env = [("GIT_INDEX_FILE", index.path().as_os_str())];
        self.git_with(&env, &["read-tree", snapshot.as_str()])?;
        // Stat data first, or every file would count as changed; it fails while files differ, which the
        // comparison after it tells.
        self.git_with(&env, &["update-index", "-q", "--refresh"])
            .ok();
        let tracked_same = self.git_says_yes(&env, &["diff-files", "--quiet"])?;
        let untracked =
            self.git_with(&env, &["ls-files", "--others", "--exclude-standard", "-z"])?;
        Ok(tracked_same && untracked.is_empty())
    }

    /// Where the folder is against `snapshot`, found without writing anything to the clone.
    pub fn on_disk(
        &self,
        change_set: &LocalChangeSet,
        snapshot: &Sha,
    ) -> Result<OnDisk, OpenRefusal> {
        let branch = change_set.branch();
        let branch_ref = format!("refs/heads/{}", branch.as_str());
        if !self.root.as_path().is_dir()
            || self
                .git(&["rev-parse", "--verify", "--quiet", &branch_ref])
                .is_err()
        {
            return Ok(OnDisk::BranchGone);
        }
        let checked_out = self
            .git(&["symbolic-ref", "--quiet", "--short", "HEAD"])
            .ok()
            .map(|name| BranchName::new(&name).map_err(|error| self.failed(error)))
            .transpose()?;
        if checked_out.as_ref() != Some(branch) {
            return Ok(OnDisk::OnAnotherBranch(checked_out));
        }
        let same = self.head()? == self.branch_head_under(snapshot)?
            && self.working_tree_matches(snapshot);
        Ok(if same { OnDisk::Same } else { OnDisk::Changed })
    }

    /// What the overview shows, as of the snapshot in `tips`: the branch's commits are those under the
    /// `HEAD` the snapshot was made over, so a commit made since does not move them.
    pub fn facts(
        &self,
        change_set: &LocalChangeSet,
        tips: &ChangeTips,
    ) -> Result<LocalFacts, OpenRefusal> {
        let branch_head = self.branch_head_under(&tips.head)?;
        let base = Sha::new(&self.git(&[
            "merge-base",
            tips.base_branch_tip.as_str(),
            branch_head.as_str(),
        ])?);
        let commit_subjects = self
            .git_untrimmed(&[
                "log",
                "--reverse",
                "--format=%s",
                &format!("{base}..{branch_head}"),
            ])?
            .lines()
            .map(str::to_owned)
            .collect();
        Ok(LocalFacts {
            repo: change_set.repo().cloned(),
            folder: self.root.clone(),
            branch: change_set.branch().clone(),
            default_branch: self.default_branch()?.name,
            commit_subjects,
            committed_files: self.changed_files(&base, &branch_head)?,
            uncommitted_files: self.changed_files(&branch_head, &tips.head)?,
        })
    }

    /// The commit a snapshot was made over: its parent when the app made it, else itself, which is
    /// `HEAD` as a clean tree's snapshot.
    fn branch_head_under(&self, snapshot: &Sha) -> Result<Sha, OpenRefusal> {
        let author = self.git(&["log", "-1", "--format=%ae", snapshot.as_str()])?;
        if author != SNAPSHOT_IDENTITY_EMAIL {
            return Ok(snapshot.clone());
        }
        Ok(Sha::new(
            &self.git(&["rev-parse", &format!("{snapshot}^")])?,
        ))
    }

    fn changed_files(&self, from: &Sha, to: &Sha) -> Result<BTreeSet<FilePath>, OpenRefusal> {
        self.git_untrimmed(&[
            "diff",
            "--name-only",
            "--no-renames",
            "-z",
            from.as_str(),
            to.as_str(),
        ])?
        .split('\0')
        .filter(|path| !path.is_empty())
        .map(|path| FilePath::new(path).map_err(|error| self.failed(error)))
        .collect()
    }

    fn head(&self) -> Result<Sha, OpenRefusal> {
        Ok(Sha::new(&self.git(&[
            "rev-parse",
            "--verify",
            "HEAD^{commit}",
        ])?))
    }

    fn tree_of(&self, commit: &str) -> Result<String, OpenRefusal> {
        self.git(&["rev-parse", "--verify", &format!("{commit}^{{tree}}")])
    }

    /// The tree id of the files on disk: `HEAD`'s tree with every change and every untracked file that
    /// is not ignored, written from an index of its own so the reviewer's index is never opened.
    /// What `changed_files` from `HEAD` to the working tree counts, from git's own index: a working tree
    /// made from a fresh index hashes every file, seconds in a big repo, and a glance is taken of many.
    fn uncommitted_file_count(&self) -> Result<usize, OpenRefusal> {
        Ok(self
            .git_untrimmed(&[
                "--no-optional-locks",
                "status",
                "--porcelain=v1",
                "-z",
                "--untracked-files=all",
                "--no-renames",
            ])?
            .split('\0')
            .filter(|entry| !entry.is_empty())
            .count())
    }

    fn working_tree_id(&self) -> Result<String, OpenRefusal> {
        let index = TemporaryIndex::new();
        let env = [("GIT_INDEX_FILE", index.path().as_os_str())];
        self.git_with(&env, &["read-tree", "HEAD"])?;
        self.git_with(&env, &["add", "--all"])?;
        self.git_with(&env, &["write-tree"])
    }

    fn git(&self, args: &[&str]) -> Result<String, OpenRefusal> {
        self.git_with(&[], args)
    }

    fn git_with(&self, env: &[(&str, &OsStr)], args: &[&str]) -> Result<String, OpenRefusal> {
        Ok(self.git_run(env, args)?.trim_end_matches('\n').to_owned())
    }

    /// For a command whose exit status is its answer: 0 is yes, 1 is no, anything else failed.
    fn git_says_yes(&self, env: &[(&str, &OsStr)], args: &[&str]) -> Result<bool, OpenRefusal> {
        let output = Command::new("git")
            .arg("-C")
            .arg(self.root.as_path())
            .args(args)
            .envs(env.iter().copied())
            .output()
            .map_err(|error| self.failed(error))?;
        match output.status.code() {
            Some(0) => Ok(true),
            Some(1) => Ok(false),
            _ => Err(self.failed(format!(
                "git {} failed: {}",
                args.join(" "),
                String::from_utf8_lossy(&output.stderr).trim()
            ))),
        }
    }

    fn git_untrimmed(&self, args: &[&str]) -> Result<String, OpenRefusal> {
        self.git_run(&[], args)
    }

    fn git_run(&self, env: &[(&str, &OsStr)], args: &[&str]) -> Result<String, OpenRefusal> {
        let output = Command::new("git")
            .arg("-C")
            .arg(self.root.as_path())
            .args(args)
            .envs(env.iter().copied())
            .output()
            .map_err(|error| self.failed(error))?;
        if !output.status.success() {
            return Err(self.failed(format!(
                "git {} failed: {}",
                args.join(" "),
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }

    fn folder(&self) -> PathBuf {
        self.root.as_path().to_path_buf()
    }

    fn failed(&self, reason: impl Display) -> OpenRefusal {
        failed(self.root.as_path(), reason)
    }
}

fn failed(folder: &Path, reason: impl Display) -> OpenRefusal {
    OpenRefusal::GitFailed {
        folder: folder.to_path_buf(),
        reason: reason.to_string(),
    }
}

/// The ref that keeps a change set's snapshot: one per change set, named by a hash of its subject key.
/// The `local-` prefix keeps it from ever being all digits, which is what a PR's fetch ref is.
pub fn snapshot_ref(change_set: &LocalChangeSet) -> String {
    let key = ReviewSubject::from(change_set.clone()).to_string();
    let digest = Sha256::digest(key.as_bytes());
    format!(
        "{SNAPSHOT_REF_PREFIX}{}",
        crate::hex::lower(digest.iter().take(SNAPSHOT_REF_HASH_BYTES))
    )
}

fn read_head_ref_prefix_of(pr: &PullRequestUrl) -> String {
    let key = ReviewSubject::from(pr.clone()).to_string();
    let digest = Sha256::digest(key.as_bytes());
    format!(
        "{READ_HEAD_REF_PREFIX}{}-",
        crate::hex::lower(digest.iter().take(SNAPSHOT_REF_HASH_BYTES))
    )
}

fn read_head_ref(pr: &PullRequestUrl, head: &Sha) -> String {
    format!("{}{}", read_head_ref_prefix_of(pr), head.as_str())
}

/// An index file in the system's temp folder, gone when this is dropped: the clone's own git folder
/// is never written to.
struct TemporaryIndex(PathBuf);

impl TemporaryIndex {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let name = format!(
            "guided-review-index-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        Self(std::env::temp_dir().join(name))
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TemporaryIndex {
    fn drop(&mut self) {
        // A leftover file is only clutter in the temp folder, and a drop cannot report it.
        std::fs::remove_file(&self.0).ok();
    }
}
