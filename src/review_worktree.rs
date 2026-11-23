//! The throwaway checkouts older builds made for the review agent, which the start-up sweep still removes.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::Context;

use crate::review_subject::ReviewSubject;

/// Git worktrees that older builds made for review agents, under one folder of the app's cache, one per
/// subject at the subject's files folder and stem. Nothing makes them any more; they are swept away.
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

pub(crate) fn git(folder: &Path, args: &[&str]) -> anyhow::Result<String> {
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
