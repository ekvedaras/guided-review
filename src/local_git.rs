use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, bail};

use crate::code_source::{BlobInfo, CodeSource, FileDiff, parse_unified_diff};
use crate::config::Config;
use crate::pull_request::{PullRequestTips, PullRequestUrl, RepoName, Sha};

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

    pub fn root(&self) -> &Path {
        &self.repo
    }

    /// Fetches what is missing into `refs/guided-review/<n>` only; branches and files stay as they are.
    pub fn fetch_missing_commits(
        &self,
        pr: &PullRequestUrl,
        tips: &PullRequestTips,
    ) -> anyhow::Result<()> {
        if !self.has_commit(&tips.head) {
            let pull_ref = format!("+refs/pull/{0}/head:refs/guided-review/{0}", pr.number);
            self.fetch(&pr.repo, &pull_ref)?;
        }
        if !self.has_commit(&tips.base_branch_tip) {
            self.fetch(&pr.repo, tips.base_branch_tip.as_str())?;
        }
        for sha in [&tips.base_branch_tip, &tips.head] {
            if !self.has_commit(sha) {
                bail!(
                    "commit {sha} is not in the clone at {}",
                    self.repo.display()
                );
            }
        }
        Ok(())
    }

    fn has_commit(&self, sha: &Sha) -> bool {
        self.git(&["cat-file", "-e", &format!("{sha}^{{commit}}")])
            .is_ok()
    }

    fn fetch(&self, repo: &RepoName, what: &str) -> anyhow::Result<()> {
        let remote = self
            .remote_names_pointing_at(repo)
            .into_iter()
            .next()
            .with_context(|| format!("no remote of {} points at {repo}", self.repo.display()))?;
        self.git(&[
            "-c",
            "gc.auto=0",
            "fetch",
            "--quiet",
            "--no-tags",
            "--no-recurse-submodules",
            &remote,
            what,
        ])
        .map(drop)
    }

    fn is_clone_of(&self, repo: &RepoName) -> bool {
        !self.remote_names_pointing_at(repo).is_empty()
    }

    fn remote_names_pointing_at(&self, repo: &RepoName) -> Vec<String> {
        let Ok(remotes) = self.git(&["config", "--get-regexp", r"^remote\..*\.url$"]) else {
            return Vec::new();
        };
        remotes
            .lines()
            .filter_map(|line| line.split_once(' '))
            .filter(|(_, url)| RepoName::from_github_remote(url.trim()).is_some_and(|r| r == *repo))
            .filter_map(|(key, _)| key.strip_prefix("remote")?.strip_suffix(".url"))
            .map(|name| name.trim_start_matches('.').to_string())
            .collect()
    }

    fn git(&self, args: &[&str]) -> anyhow::Result<String> {
        Ok(String::from_utf8_lossy(&self.git_bytes(args)?).into_owned())
    }

    fn git_bytes(&self, args: &[&str]) -> anyhow::Result<Vec<u8>> {
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
        Ok(output.stdout)
    }
}

impl CodeSource for LocalGit {
    fn local_clone(&self) -> Option<&Path> {
        Some(&self.repo)
    }

    fn file_at(&self, sha: &Sha, path: &str) -> anyhow::Result<String> {
        self.git(&["show", &format!("{sha}:{path}")])
    }

    fn blob_at(&self, sha: &Sha, path: &str) -> anyhow::Result<Vec<u8>> {
        self.git_bytes(&["show", &format!("{sha}:{path}")])
    }

    fn blob_info_at(&self, sha: &Sha, path: &str) -> anyhow::Result<BlobInfo> {
        let listed = self.git(&["ls-tree", "-l", "--full-tree", sha.as_str(), "--", path])?;
        let mut fields = listed.split_whitespace();
        let (id, size) = (fields.nth(2), fields.next());
        match (id, size.and_then(|size| size.parse().ok())) {
            (Some(id), Some(size)) => Ok(BlobInfo {
                id: id.to_string(),
                size,
            }),
            _ => bail!("{path} is not a file at {sha}"),
        }
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

    fn working_tree_is_at(&self, head: &Sha) -> bool {
        let checked_out = self.git(&["rev-parse", "HEAD"]).map(|sha| Sha::new(&sha));
        let uncommitted = self.git(&["status", "--porcelain", "--untracked-files=no"]);
        checked_out.is_ok_and(|sha| sha == *head)
            && uncommitted.is_ok_and(|status| status.is_empty())
    }
}
