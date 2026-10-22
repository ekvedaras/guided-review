use std::process::Command;

use anyhow::Context;
use serde::Deserialize;

use crate::code_source::{CodeSource, FileDiff, parse_unified_diff};
use crate::pull_request::{PullRequestTips, PullRequestUrl, RepoName, Sha};

#[derive(Clone)]
pub struct GitHub {
    api: String,
    token: Option<String>,
}

#[derive(Deserialize)]
struct PullJson {
    base: CommitRefJson,
    head: CommitRefJson,
}

#[derive(Deserialize)]
struct CommitRefJson {
    sha: String,
}

impl GitHub {
    pub fn from_env() -> Self {
        Self {
            api: std::env::var("GUIDED_REVIEW_GITHUB_API")
                .unwrap_or_else(|_| "https://api.github.com".to_string()),
            token: token_from_gh_cli().or_else(|| std::env::var("GITHUB_TOKEN").ok()),
        }
    }

    pub fn pull_request_tips(&self, pr: &PullRequestUrl) -> anyhow::Result<PullRequestTips> {
        let pull: PullJson = self
            .get(
                &format!("repos/{}/pulls/{}", pr.repo, pr.number),
                "application/vnd.github+json",
            )
            .and_then(|mut response| Ok(response.body_mut().read_json()?))
            .with_context(|| format!("could not read {pr} from GitHub"))?;
        Ok(PullRequestTips {
            base_branch_tip: Sha::new(&pull.base.sha),
            head: Sha::new(&pull.head.sha),
        })
    }

    fn get(&self, path: &str, accept: &str) -> anyhow::Result<ureq::http::Response<ureq::Body>> {
        let mut request = ureq::get(format!("{}/{path}", self.api))
            .header("Accept", accept)
            .header("User-Agent", "guided-review");
        if let Some(token) = &self.token {
            request = request.header("Authorization", format!("Bearer {token}"));
        }
        Ok(request.call()?)
    }

    pub fn code_source(&self, repo: &RepoName) -> GitHubCodeSource {
        GitHubCodeSource {
            github: self.clone(),
            repo: repo.clone(),
        }
    }
}

/// Reads one repo through the GitHub API, for PRs without a local clone.
pub struct GitHubCodeSource {
    github: GitHub,
    repo: RepoName,
}

#[derive(Deserialize)]
struct CompareJson {
    merge_base_commit: CommitRefJson,
}

impl GitHubCodeSource {
    fn compare_path(&self, base: &Sha, head: &Sha) -> String {
        format!("repos/{}/compare/{base}...{head}", self.repo)
    }
}

impl CodeSource for GitHubCodeSource {
    fn file_at(&self, sha: &Sha, path: &str) -> anyhow::Result<String> {
        Ok(String::from_utf8_lossy(&self.blob_at(sha, path)?).into_owned())
    }

    fn blob_at(&self, sha: &Sha, path: &str) -> anyhow::Result<Vec<u8>> {
        let url = format!(
            "repos/{}/contents/{}?ref={sha}",
            self.repo,
            percent_encode_path(path)
        );
        let mut response = self
            .github
            .get(&url, "application/vnd.github.raw+json")
            .with_context(|| format!("could not read {path} at {sha} from GitHub"))?;
        Ok(response.body_mut().read_to_vec()?)
    }

    fn diff(&self, base: &Sha, head: &Sha) -> anyhow::Result<Vec<FileDiff>> {
        let patch = self
            .github
            .get(
                &self.compare_path(base, head),
                "application/vnd.github.diff",
            )
            .and_then(|mut response| Ok(response.body_mut().read_to_string()?))
            .map_err(|error| match error.downcast_ref::<ureq::Error>() {
                Some(ureq::Error::StatusCode(406 | 422)) => anyhow::anyhow!(
                    "GitHub refuses to send the diff {base}...{head} because it is too large; \
                     use a local clone of {} (run from inside it, or list it under [clones] in the config)",
                    self.repo
                ),
                _ => error.context(format!("could not read the diff {base}...{head} from GitHub")),
            })?;
        Ok(parse_unified_diff(&patch))
    }

    fn merge_base(&self, a: &Sha, b: &Sha) -> anyhow::Result<Sha> {
        let compare: CompareJson = self
            .github
            .get(&self.compare_path(a, b), "application/vnd.github+json")
            .and_then(|mut response| Ok(response.body_mut().read_json()?))
            .with_context(|| format!("could not find the merge base of {a} and {b} on GitHub"))?;
        Ok(Sha::new(&compare.merge_base_commit.sha))
    }

    fn working_tree_is_at(&self, _head: &Sha) -> bool {
        false
    }
}

fn percent_encode_path(path: &str) -> String {
    path.bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => {
                char::from(byte).to_string()
            }
            _ => format!("%{byte:02X}"),
        })
        .collect()
}

fn token_from_gh_cli() -> Option<String> {
    let output = Command::new("gh").args(["auth", "token"]).output().ok()?;
    let token = String::from_utf8(output.stdout).ok()?.trim().to_string();
    (output.status.success() && !token.is_empty()).then_some(token)
}
