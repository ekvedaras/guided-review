//! The throwaway checkout the review agent works in: the reviewed head, outside the clone, so the agent
//! never touches the reviewer's files and a build it runs never lands in their folder.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::Context;

use crate::pull_request::Sha;
use crate::review_subject::ReviewSubject;

/// A checkout made for one review agent, gone when the agent ends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewWorktree(PathBuf);

impl ReviewWorktree {
    pub fn at(path: PathBuf) -> Self {
        Self(path)
    }

    pub fn as_path(&self) -> &Path {
        &self.0
    }
}

pub trait ReviewWorktreePort: Send + Sync {
    /// Checks out `head` of the repository at `repo` for the subject's review agent, replacing a
    /// checkout an earlier run left.
    fn add(
        &self,
        subject: &ReviewSubject,
        repo: &Path,
        head: &Sha,
    ) -> anyhow::Result<ReviewWorktree>;

    /// Deletes the subject's checkout; one that is not there is already gone.
    fn remove(&self, subject: &ReviewSubject) -> anyhow::Result<()>;
}

/// Git worktrees under one folder of the app's cache, one per subject at the subject's files folder and
/// stem, so a checkout names its review.
pub struct GitReviewWorktrees {
    root: PathBuf,
}

impl GitReviewWorktrees {
    pub fn under(root: PathBuf) -> Self {
        Self { root }
    }

    fn path_of(&self, subject: &ReviewSubject) -> PathBuf {
        self.root
            .join(subject.files_folder())
            .join(subject.file_stem())
    }

    /// The commit the subject's checkout stands at: the head its review agent reads, which a PR may
    /// have moved past since. `None` when there is no checkout.
    pub fn head_of(&self, subject: &ReviewSubject) -> Option<Sha> {
        let path = self.path_of(subject);
        path.exists()
            .then(|| git(&path, &["rev-parse", "HEAD"]).ok())
            .flatten()
            .map(|head| Sha::new(&head))
    }

    /// Removes every checkout under the folder but the kept subjects', as after a crash that left
    /// one behind. Says what it could not remove.
    pub fn sweep(&self, keep: &[ReviewSubject]) -> Vec<String> {
        let kept: Vec<PathBuf> = keep.iter().map(|subject| self.path_of(subject)).collect();
        let mut not_swept = Vec::new();
        for checkout in checkouts_under(&self.root) {
            if kept.contains(&checkout) {
                continue;
            }
            if let Err(error) = remove_checkout(&checkout) {
                not_swept.push(format!("{error:#}"));
            }
        }
        not_swept
    }
}

impl ReviewWorktreePort for GitReviewWorktrees {
    fn add(
        &self,
        subject: &ReviewSubject,
        repo: &Path,
        head: &Sha,
    ) -> anyhow::Result<ReviewWorktree> {
        anyhow::ensure!(
            git(repo, &["rev-parse", "--git-dir"]).is_ok(),
            "There is no clone on this Mac to check the code of {subject} out of: open the PR from a clone of its repository, then run the AI review again"
        );
        let path = self.path_of(subject);
        remove_checkout(&path)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("could not create {}", parent.display()))?;
        }
        git(repo, &["worktree", "prune"])?;
        let path_text = path.to_string_lossy();
        git(
            repo,
            &["worktree", "add", "--detach", &path_text, head.as_str()],
        )?;
        Ok(ReviewWorktree(path))
    }

    fn remove(&self, subject: &ReviewSubject) -> anyhow::Result<()> {
        remove_checkout(&self.path_of(subject))
    }
}

/// The folders under `root` that git made as worktrees: the ones with a `.git` file in them.
fn checkouts_under(root: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.join(".git").exists() {
            found.push(path);
        } else if path.is_dir() {
            found.extend(checkouts_under(&path));
        }
    }
    found
}

/// Has the repository the checkout belongs to forget it, then deletes the folder; a folder that is not
/// there is done. A checkout whose repository is gone is only deleted.
fn remove_checkout(path: &Path) -> anyhow::Result<()> {
    if !path.exists() {
        return Ok(());
    }
    if let Ok(common) = git(
        path,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    ) {
        let path_text = path.to_string_lossy();
        let common = PathBuf::from(common.trim());
        let forgotten = Command::new("git")
            .arg("--git-dir")
            .arg(&common)
            .args(["worktree", "remove", "--force", &path_text])
            .output();
        if forgotten.is_ok_and(|output| output.status.success()) {
            return Ok(());
        }
    }
    std::fs::remove_dir_all(path).with_context(|| format!("could not delete {}", path.display()))
}

fn git(folder: &Path, args: &[&str]) -> anyhow::Result<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(folder)
        .args(args)
        .output()
        .context("could not run git")?;
    anyhow::ensure!(
        output.status.success(),
        "git {} failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr).trim()
    );
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}
