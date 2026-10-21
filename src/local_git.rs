use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, bail};

use crate::code_source::{CodeSource, FileDiff, parse_unified_diff};
use crate::config::Config;
use crate::pull_request::{RepoName, Sha};

/// Reads git objects only, never the working tree, so uncommitted work never leaks into a tour.
pub struct LocalGit {
    repo: PathBuf,
}

impl LocalGit {
    pub fn find(repo: &RepoName, current_dir: &Path) -> anyhow::Result<Option<Self>> {
        let current = Self {
            repo: current_dir.to_path_buf(),
        };
        if current.is_clone_of(repo) {
            return Ok(Some(current));
        }
        let Some(configured) = Config::load()?.clone_of(repo) else {
            return Ok(None);
        };
        let configured = Self { repo: configured };
        if !configured.is_clone_of(repo) {
            bail!(
                "~/.guided-review/config.toml says {} is a clone of {repo}, but no remote there points at {repo}",
                configured.repo.display()
            );
        }
        Ok(Some(configured))
    }

    pub fn require_commits(&self, shas: &[&Sha]) -> anyhow::Result<()> {
        for sha in shas {
            let commit = format!("{sha}^{{commit}}");
            if self.git(&["cat-file", "-e", &commit]).is_err() {
                bail!(
                    "commit {sha} is not in the clone at {}; run `git fetch` there",
                    self.repo.display()
                );
            }
        }
        Ok(())
    }

    fn is_clone_of(&self, repo: &RepoName) -> bool {
        self.git(&["config", "--get-regexp", r"^remote\..*\.url$"])
            .is_ok_and(|remotes| {
                remotes
                    .lines()
                    .filter_map(|line| line.split_whitespace().nth(1))
                    .filter_map(RepoName::from_github_remote)
                    .any(|remote| remote == *repo)
            })
    }

    fn git(&self, args: &[&str]) -> anyhow::Result<String> {
        let output = Command::new("git")
            .arg("-C")
            .arg(&self.repo)
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
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }
}

impl CodeSource for LocalGit {
    fn file_at(&self, sha: &Sha, path: &str) -> anyhow::Result<String> {
        self.git(&["show", &format!("{sha}:{path}")])
    }

    fn diff(&self, base: &Sha, head: &Sha) -> anyhow::Result<Vec<FileDiff>> {
        let patch = self.git(&[
            "-c",
            "core.quotePath=false",
            "diff",
            "-U0",
            "-M",
            "--no-color",
            "--no-ext-diff",
            "--no-textconv",
            "--src-prefix=a/",
            "--dst-prefix=b/",
            base.as_str(),
            head.as_str(),
        ])?;
        Ok(parse_unified_diff(&patch))
    }

    fn merge_base(&self, a: &Sha, b: &Sha) -> anyhow::Result<Sha> {
        Ok(Sha::new(&self.git(&[
            "merge-base",
            a.as_str(),
            b.as_str(),
        ])?))
    }
}
