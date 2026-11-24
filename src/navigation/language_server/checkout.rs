//! The server checkout: the app's own worktree of a repo, which every language server of the repo
//! runs in, moved to each reviewed head.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context as _, bail};

use crate::codebase::Codebase;
use crate::file_path::FilePath;
use crate::pull_request::Sha;

/// Where the server checkouts live, one folder per codebase.
pub struct ServerCheckouts {
    folder: PathBuf,
}

/// A server checkout at a head.
#[derive(Debug)]
pub struct Checkout {
    pub folder: PathBuf,
    /// What the move changed in the files the server may have read, for the server to hear.
    pub changed: Vec<ChangedFile>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangedFile {
    pub path: FilePath,
    pub change: Change,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    Created,
    Changed,
    Deleted,
}

impl ServerCheckouts {
    pub fn new(folder: PathBuf) -> Self {
        Self { folder }
    }

    /// Makes the repo's checkout when it has none, and checks it out detached at `head`. The clone's
    /// files, index and branch are never touched.
    pub fn moved_to(
        &self,
        clone: &Path,
        codebase: &Codebase,
        head: &Sha,
    ) -> anyhow::Result<Checkout> {
        let folder = self.folder.join(codebase.state_folder());
        let mut changed = Vec::new();
        if folder.join(".git").exists() {
            let at = git(&folder, &["rev-parse", "HEAD"])?;
            if at != head.as_str() {
                changed = changes_between(&folder, &at, head)?;
                unlink_what_head_tracks(&folder, head)?;
                git(&folder, &["checkout", "--quiet", "--detach", head.as_str()])?;
            }
        } else {
            if let Some(parent) = folder.parent() {
                std::fs::create_dir_all(parent)
                    .with_context(|| format!("could not make {}", parent.display()))?;
            }
            git(
                clone,
                &[
                    "worktree",
                    "add",
                    "--detach",
                    "--force",
                    &path_text(&folder),
                    head.as_str(),
                ],
            )?;
        }
        borrow_dependencies(clone, &folder, head)?;
        Ok(Checkout { folder, changed })
    }
}

fn changes_between(folder: &Path, from: &str, to: &Sha) -> anyhow::Result<Vec<ChangedFile>> {
    let listed = git(
        folder,
        &[
            "diff",
            "--name-status",
            "--no-renames",
            "-z",
            from,
            to.as_str(),
        ],
    )?;
    let mut fields = listed.split('\0');
    let mut changed = Vec::new();
    while let (Some(status), Some(path)) = (fields.next(), fields.next()) {
        let change = match status {
            "A" => Change::Created,
            "D" => Change::Deleted,
            _ => Change::Changed,
        };
        changed.push(ChangedFile {
            path: FilePath::new(path)?,
            change,
        });
    }
    Ok(changed)
}

/// A link in the way of files the head holds would make the move fail.
fn unlink_what_head_tracks(folder: &Path, head: &Sha) -> anyhow::Result<()> {
    for name in BORROWED {
        let link = folder.join(name);
        if link.is_symlink() && tracks(folder, head, name)? {
            std::fs::remove_file(&link)
                .with_context(|| format!("could not remove the link {}", link.display()))?;
        }
    }
    Ok(())
}

fn tracks(folder: &Path, head: &Sha, name: &str) -> anyhow::Result<bool> {
    Ok(!git(folder, &["ls-tree", head.as_str(), "--", name])?.is_empty())
}

/// Installed packages are not in git, so the checkout links the clone's folders instead of copying them.
const BORROWED: [&str; 2] = ["vendor", "node_modules"];

fn borrow_dependencies(clone: &Path, folder: &Path, head: &Sha) -> anyhow::Result<()> {
    for name in BORROWED {
        let from = clone.join(name);
        let to = folder.join(name);
        let head_tracks_it = tracks(folder, head, name)?;
        if !from.is_dir() || to.symlink_metadata().is_ok() || head_tracks_it {
            continue;
        }
        std::os::unix::fs::symlink(&from, &to).with_context(|| {
            format!("could not link {} into the server checkout", from.display())
        })?;
    }
    Ok(())
}

fn path_text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// The PR's files may hold hooks that a relative `core.hooksPath` would point at, so no git command on
/// a server checkout runs any.
fn git(folder: &Path, args: &[&str]) -> anyhow::Result<String> {
    let output = Command::new("git")
        .args(["-c", "core.hooksPath=/dev/null"])
        .arg("-C")
        .arg(folder)
        .args(args)
        .output()
        .context("could not run git")?;
    if !output.status.success() {
        bail!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}
