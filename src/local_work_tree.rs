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
use crate::pull_request::{ChangeTips, Sha};
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
        branch: String,
        default_branch: String,
    },
    #[error("git failed in {}: {reason}", .folder.display())]
    GitFailed { folder: PathBuf, reason: String },
}

/// The branch a change set is compared with, and where it is.
#[derive(Debug, Clone)]
pub struct DefaultBranch {
    pub name: String,
    pub tip: Sha,
}

/// The commit a local review reads: a commit of the working tree, or `HEAD` itself when it is clean.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    pub head: Sha,
}

/// The git adapter of a local review: the one place that runs git in the reviewer's own working tree.
/// It never changes their index, working tree, branches or stash; its only write is the snapshot's ref
/// and the commit object behind it.
#[derive(Debug, Clone)]
pub struct LocalWorkTree {
    root: WorkTreeRoot,
}

const SNAPSHOT_REF_PREFIX: &str = "refs/guided-review/local-";
/// Six bytes are the 12 hex characters that tell one local review's ref from another's.
const SNAPSHOT_REF_HASH_BYTES: usize = 6;
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
                    name,
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
            branch: change_set.branch().as_str().to_owned(),
            default_branch: default.name,
        })
    }

    /// Makes the commit of the working tree and keeps it with the change set's ref, moving the ref
    /// when there was one. The commit has `HEAD` as its parent, from a temporary index; with a clean
    /// working tree it is `HEAD` itself and nothing is committed.
    pub fn snapshot(&self, change_set: &LocalChangeSet) -> Result<Snapshot, OpenRefusal> {
        let head = self.head()?;
        let tree = self.working_tree_id()?;
        let commit = if tree == self.tree_of("HEAD")? {
            head
        } else {
            let identity = [
                ("GIT_AUTHOR_NAME", SNAPSHOT_IDENTITY_NAME),
                ("GIT_AUTHOR_EMAIL", SNAPSHOT_IDENTITY_EMAIL),
                ("GIT_COMMITTER_NAME", SNAPSHOT_IDENTITY_NAME),
                ("GIT_COMMITTER_EMAIL", SNAPSHOT_IDENTITY_EMAIL),
            ]
            .map(|(name, value)| (name, OsStr::new(value)));
            Sha::new(&self.git_with(
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
            )?)
        };
        self.git(&["update-ref", &snapshot_ref(change_set), commit.as_str()])?;
        Ok(Snapshot { head: commit })
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
        match (self.working_tree_id(), self.tree_of(snapshot.as_str())) {
            (Ok(on_disk), Ok(snapshot_tree)) => on_disk == snapshot_tree,
            _ => false,
        }
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
    fn working_tree_id(&self) -> Result<String, OpenRefusal> {
        let index = TemporaryIndex::in_git_dir_of(self)?;
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

/// An index file in the clone's own git folder, gone when this is dropped.
struct TemporaryIndex(PathBuf);

impl TemporaryIndex {
    fn in_git_dir_of(work_tree: &LocalWorkTree) -> Result<Self, OpenRefusal> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let git_dir = work_tree.git(&["rev-parse", "--absolute-git-dir"])?;
        let name = format!(
            "guided-review-index-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        Ok(Self(Path::new(&git_dir).join(name)))
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TemporaryIndex {
    fn drop(&mut self) {
        // A leftover file is only clutter in the git folder, and a drop cannot report it.
        std::fs::remove_file(&self.0).ok();
    }
}
