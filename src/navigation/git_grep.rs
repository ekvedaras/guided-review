use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context as _, bail};

use crate::file_path::FilePath;
use crate::language::Language;
use crate::pull_request::Sha;

use super::{FilesWithWord, Word};

/// Searches a clone's git objects, so no checkout of the commit is needed.
pub struct GitGrep {
    clone: PathBuf,
}

impl GitGrep {
    pub fn in_clone(clone: &Path) -> Self {
        Self {
            clone: clone.to_path_buf(),
        }
    }
}

const NO_MATCH: i32 = 1;

impl FilesWithWord for GitGrep {
    fn files_with(&self, sha: &Sha, word: &Word) -> anyhow::Result<Vec<FilePath>> {
        let output = Command::new("git")
            .arg("-C")
            .arg(&self.clone)
            .args([
                "grep",
                "--files-with-matches",
                "--null",
                "--word-regexp",
                "--fixed-strings",
                "-I",
                "-e",
                word.as_str(),
                sha.as_str(),
                "--",
            ])
            .args(Language::file_globs())
            .output()
            .context("could not run git grep")?;
        if output.status.code() == Some(NO_MATCH) {
            return Ok(Vec::new());
        }
        if !output.status.success() {
            bail!(
                "git grep for {word} at {sha} failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        let listed = String::from_utf8_lossy(&output.stdout);
        let in_commit = format!("{sha}:");
        let mut files = listed
            .split('\0')
            .filter_map(|line| line.strip_prefix(&in_commit))
            .map(FilePath::new)
            .collect::<Result<Vec<_>, _>>()
            .with_context(|| format!("git grep for {word} named a path it should not"))?;
        files.sort();
        Ok(files)
    }
}
