use std::process::Command;

use anyhow::Context;
use serde::Deserialize;

use crate::pull_request::{PullRequestTips, PullRequestUrl, Sha};

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
        let url = format!("{}/repos/{}/pulls/{}", self.api, pr.repo, pr.number);
        let mut request = ureq::get(&url)
            .header("Accept", "application/vnd.github+json")
            .header("User-Agent", "guided-review");
        if let Some(token) = &self.token {
            request = request.header("Authorization", format!("Bearer {token}"));
        }
        let pull: PullJson = request
            .call()
            .and_then(|mut response| response.body_mut().read_json())
            .with_context(|| format!("could not read {pr} from GitHub"))?;
        Ok(PullRequestTips {
            base_branch_tip: Sha::new(&pull.base.sha),
            head: Sha::new(&pull.head.sha),
        })
    }
}

fn token_from_gh_cli() -> Option<String> {
    let output = Command::new("gh").args(["auth", "token"]).output().ok()?;
    let token = String::from_utf8(output.stdout).ok()?.trim().to_string();
    (output.status.success() && !token.is_empty()).then_some(token)
}
