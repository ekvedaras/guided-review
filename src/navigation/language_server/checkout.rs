//! The server checkout: the app's own git repo of a codebase, which every language server of the
//! codebase runs in, moved to each reviewed head. It reads its objects from the review's clone and
//! records nothing in it.

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

const EARLIER_LOCAL_FOLDERS: &str = "local folder";

/// A server checkout at a head.
#[derive(Debug)]
pub struct Checkout {
    pub folder: PathBuf,
    pub head: Sha,
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

impl Change {
    fn from_status(status: &str) -> anyhow::Result<Self> {
        match status {
            "A" => Ok(Self::Created),
            "D" => Ok(Self::Deleted),
            "M" | "T" => Ok(Self::Changed),
            other => bail!("git diff gave the unknown status {other}"),
        }
    }
}

impl ServerCheckouts {
    pub fn new(folder: PathBuf) -> Self {
        Self { folder }
    }

    /// Makes the codebase's checkout when it has none, fills it with `head` from `clone` and checks
    /// `head` out detached. Nothing is written in the clone.
    pub fn moved_to(
        &self,
        clone: &Path,
        codebase: &Codebase,
        head: &Sha,
    ) -> anyhow::Result<Checkout> {
        let folder = self.folder.join(codebase.state_folder());
        let source = ReviewClone::at(clone)?;
        remove_earlier_worktree(&folder)?;
        let repo = OwnRepo::opened(&folder, &source)?;
        repo.fill_with(&source, head)?;
        repo.keep_out_program_settings()?;
        unlink_what_head_tracks(&folder, head)?;
        git(&folder, &["checkout", "--quiet", "--detach", head.as_str()])?;
        borrow_dependencies(clone, &folder, head)?;
        Ok(Checkout {
            folder,
            head: head.clone(),
        })
    }
}

impl ServerCheckouts {
    /// Builds of this branch before the Codebase key kept a local folder's checkout and Intelephense
    /// index under `local folder/`, which nothing reads any more.
    pub fn remove_local_folder_leftovers(&self, server_indexes: &Path) -> anyhow::Result<()> {
        let checkouts = self.folder.join(EARLIER_LOCAL_FOLDERS);
        if checkouts.is_dir() {
            let entries = std::fs::read_dir(&checkouts)
                .with_context(|| format!("could not list {}", checkouts.display()))?;
            for entry in entries {
                let entry =
                    entry.with_context(|| format!("could not list {}", checkouts.display()))?;
                remove_earlier_worktree(&entry.path())?;
            }
        }
        for leftover in [checkouts, server_indexes.join(EARLIER_LOCAL_FOLDERS)] {
            if leftover.symlink_metadata().is_ok() {
                std::fs::remove_dir_all(&leftover)
                    .with_context(|| format!("could not remove {}", leftover.display()))?;
            }
        }
        Ok(())
    }
}

impl Checkout {
    /// What changed in the files a server may have read since it last heard of `heard`.
    pub fn changes_since(&self, heard: &Sha) -> anyhow::Result<Vec<ChangedFile>> {
        let left_out = PROGRAM_SETTINGS.map(|name| format!(":(exclude,glob)**/{name}"));
        let mut args = vec![
            "diff",
            "--name-status",
            "--no-renames",
            "-z",
            heard.as_str(),
            self.head.as_str(),
            "--",
        ];
        args.extend(left_out.iter().map(String::as_str));
        let listed = git(&self.folder, &args)?;
        let mut fields = listed.split('\0').filter(|field| !field.is_empty());
        let mut changed = Vec::new();
        while let (Some(status), Some(path)) = (fields.next(), fields.next()) {
            changed.push(ChangedFile {
                path: FilePath::new(path)?,
                change: Change::from_status(status)?,
            });
        }
        Ok(changed)
    }
}

/// The reviewer's clone a review reads, which the checkout takes its objects from.
struct ReviewClone<'a> {
    folder: &'a Path,
    objects: PathBuf,
}

impl<'a> ReviewClone<'a> {
    fn at(folder: &'a Path) -> anyhow::Result<Self> {
        let objects = git(
            folder,
            &[
                "rev-parse",
                "--path-format=absolute",
                "--git-path",
                "objects",
            ],
        )?;
        let objects = Path::new(&objects)
            .canonicalize()
            .with_context(|| format!("could not find the objects of {}", folder.display()))?;
        Ok(Self { folder, objects })
    }
}

/// Files in the PR's tree that make a language server, or the cargo or rustup it runs, start a
/// program the PR names (tried against the real servers, see docs/wiki/definitions-on-hover.md).
const PROGRAM_SETTINGS: [&str; 6] = [
    ".cargo/config.toml",
    ".cargo/config",
    "rust-toolchain.toml",
    "rust-toolchain",
    "rust-project.json",
    // No key of it ran anything with rust-analyzer 2026-11-23, but the keys a workspace may set
    // change between releases.
    "rust-analyzer.toml",
];

/// Where the checkout's objects are: read from the clone it was made from, or copied into its own.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Objects {
    Borrowed,
    Own,
}

struct OwnRepo<'a> {
    folder: &'a Path,
}

impl<'a> OwnRepo<'a> {
    /// The checkout's repo, made anew when it has none or can no longer read its head. A checkout that
    /// borrows the objects of another clone, or of one that is gone, is made anew with its own objects,
    /// so it never points at objects that may go missing.
    fn opened(folder: &'a Path, source: &ReviewClone<'_>) -> anyhow::Result<Self> {
        if !folder.join(".git").is_dir() {
            return Self::made(folder, source, Objects::Borrowed);
        }
        let objects = match borrowed_from(folder)? {
            Some(objects) if objects != source.objects => {
                return Self::made(folder, source, Objects::Own);
            }
            Some(_) => Objects::Borrowed,
            None => Objects::Own,
        };
        if reads_its_head(folder) {
            Ok(Self { folder })
        } else {
            Self::made(folder, source, objects)
        }
    }

    fn made(folder: &'a Path, source: &ReviewClone<'_>, objects: Objects) -> anyhow::Result<Self> {
        if folder.symlink_metadata().is_ok() {
            std::fs::remove_dir_all(folder)
                .with_context(|| format!("could not remove {}", folder.display()))?;
        }
        std::fs::create_dir_all(folder)
            .with_context(|| format!("could not make {}", folder.display()))?;
        // An empty template leaves the repo with no hooks folder.
        git(folder, &["init", "--quiet", "--template="])?;
        if objects == Objects::Borrowed {
            let info = folder.join(".git/objects/info");
            std::fs::create_dir_all(&info)
                .with_context(|| format!("could not make {}", info.display()))?;
            std::fs::write(
                info.join("alternates"),
                format!("{}\n", source.objects.display()),
            )
            .with_context(|| format!("could not point {} at the clone", folder.display()))?;
        }
        Ok(Self { folder })
    }

    /// A sparse checkout leaves the program settings out of the folder wherever they are in the tree,
    /// and takes them out of a checkout an earlier build filled.
    fn keep_out_program_settings(&self) -> anyhow::Result<()> {
        let left_out = PROGRAM_SETTINGS.map(|name| format!("!**/{name}"));
        let mut args = vec!["sparse-checkout", "set", "--no-cone", "/*"];
        args.extend(left_out.iter().map(String::as_str));
        git(self.folder, &args).map(drop)
    }

    fn fill_with(&self, source: &ReviewClone<'_>, head: &Sha) -> anyhow::Result<()> {
        let commit = format!("{}^{{commit}}", head.as_str());
        if git(self.folder, &["cat-file", "-e", &commit]).is_ok() {
            return Ok(());
        }
        git(
            self.folder,
            &[
                "fetch",
                "--quiet",
                "--no-tags",
                "--no-write-fetch-head",
                &path_text(source.folder),
                head.as_str(),
            ],
        )
        .map(drop)
    }
}

/// The objects folder the checkout reads from beside its own, if any.
fn borrowed_from(folder: &Path) -> anyhow::Result<Option<PathBuf>> {
    let alternates = folder.join(".git/objects/info/alternates");
    if !alternates.exists() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(&alternates)
        .with_context(|| format!("could not read {}", alternates.display()))?;
    let named = PathBuf::from(text.trim());
    // A clone that is gone matches no clone.
    Ok(Some(named.canonicalize().unwrap_or(named)))
}

fn reads_its_head(folder: &Path) -> bool {
    git(folder, &["ls-tree", "-r", "-t", "--name-only", "HEAD"]).is_ok()
}

/// Builds of this branch before the checkout was the app's own repo made it a worktree of the
/// reviewer's clone: its entry in the clone is removed with it.
fn remove_earlier_worktree(folder: &Path) -> anyhow::Result<()> {
    let link = folder.join(".git");
    if !link.is_file() {
        return Ok(());
    }
    let text = std::fs::read_to_string(&link)
        .with_context(|| format!("could not read {}", link.display()))?;
    let worktree_entry = text.trim().strip_prefix("gitdir:").map(str::trim);
    let clone_git = worktree_entry
        .map(Path::new)
        .and_then(Path::parent)
        .and_then(Path::parent)
        .filter(|git_dir| git_dir.is_dir());
    if let Some(clone_git) = clone_git {
        // The folder is removed below whatever the clone says, and prune then drops the entry.
        git_in(
            clone_git,
            &["worktree", "remove", "--force", &path_text(folder)],
        )
        .ok();
    }
    if folder.symlink_metadata().is_ok() {
        std::fs::remove_dir_all(folder)
            .with_context(|| format!("could not remove {}", folder.display()))?;
    }
    if let Some(clone_git) = clone_git {
        git_in(clone_git, &["worktree", "prune"])?;
    }
    Ok(())
}

/// A link in the way of files the head holds would make the move fail.
fn unlink_what_head_tracks(folder: &Path, head: &Sha) -> anyhow::Result<()> {
    for name in BORROWED {
        let link = folder.join(name);
        if link.is_symlink() && tracks(folder, head, name)? {
            remove_link(&link)?;
        }
    }
    Ok(())
}

fn tracks(folder: &Path, head: &Sha, name: &str) -> anyhow::Result<bool> {
    Ok(!git(folder, &["ls-tree", head.as_str(), "--", name])?.is_empty())
}

/// Installed packages are not in git, so the checkout links the clone's folders instead of copying them.
const BORROWED: [&str; 2] = ["vendor", "node_modules"];

/// Links the review's own clone's folders, and re-points links an earlier review's clone left.
fn borrow_dependencies(clone: &Path, folder: &Path, head: &Sha) -> anyhow::Result<()> {
    for name in BORROWED {
        let from = clone.join(name);
        let to = folder.join(name);
        if to.is_symlink() && std::fs::read_link(&to).ok().as_deref() != Some(from.as_path()) {
            remove_link(&to)?;
        }
        if !from.is_dir() || to.symlink_metadata().is_ok() || tracks(folder, head, name)? {
            continue;
        }
        std::os::unix::fs::symlink(&from, &to).with_context(|| {
            format!("could not link {} into the server checkout", from.display())
        })?;
    }
    Ok(())
}

fn remove_link(link: &Path) -> anyhow::Result<()> {
    std::fs::remove_file(link)
        .with_context(|| format!("could not remove the link {}", link.display()))
}

fn path_text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// The PR's files may hold hooks that a relative `core.hooksPath` would point at, so no git command on
/// a server checkout runs any.
fn git(folder: &Path, args: &[&str]) -> anyhow::Result<String> {
    run_git(Command::new("git").arg("-C").arg(folder), args)
}

fn git_in(git_dir: &Path, args: &[&str]) -> anyhow::Result<String> {
    run_git(Command::new("git").arg("--git-dir").arg(git_dir), args)
}

fn run_git(command: &mut Command, args: &[&str]) -> anyhow::Result<String> {
    let output = command
        .args(["-c", "core.hooksPath=/dev/null"])
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
