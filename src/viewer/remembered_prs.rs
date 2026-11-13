use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::{Deserialize, Serialize};

use super::relevant_prs::{RelevantPullRequest, WhyListed};

/// `~/.guided-review/state/pull-requests.json`: the switcher's last list from GitHub, shown at once
/// on the next start while GitHub is read again.
#[derive(Clone)]
pub struct RememberedPullRequestsFile {
    path: PathBuf,
}

#[derive(Serialize, Deserialize)]
struct Remembered {
    url: String,
    title: String,
    author: String,
    is_draft: bool,
    updated_at: String,
    why_listed: RememberedWhy,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum RememberedWhy {
    Mine,
    ReviewRequested,
    Involved,
}

impl RememberedPullRequestsFile {
    pub fn in_home(home: &Path) -> Self {
        Self {
            path: home.join(".guided-review/state/pull-requests.json"),
        }
    }

    /// No file yet is an empty list. A file that cannot be read as a list is moved to
    /// `pull-requests.broken.json`, so it can be looked at and the next save does not write over it.
    pub(super) fn load(&self) -> anyhow::Result<Vec<RelevantPullRequest>> {
        let text = match std::fs::read_to_string(&self.path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("could not read {}", self.path.display()));
            }
        };
        Self::parse(&text).or_else(|not_valid| {
            let backup = self.path.with_file_name("pull-requests.broken.json");
            std::fs::rename(&self.path, &backup).with_context(|| {
                format!(
                    "{} is not valid ({not_valid:#}) and could not be moved aside",
                    self.path.display()
                )
            })?;
            anyhow::bail!(
                "{} is not valid ({not_valid:#}); moved to {}",
                self.path.display(),
                backup.display()
            )
        })
    }

    fn parse(text: &str) -> anyhow::Result<Vec<RelevantPullRequest>> {
        let remembered: Vec<Remembered> = serde_json::from_str(text)?;
        remembered
            .into_iter()
            .map(|pull_request| {
                Ok(RelevantPullRequest {
                    url: pull_request.url.parse()?,
                    title: pull_request.title,
                    author: pull_request.author,
                    is_draft: pull_request.is_draft,
                    updated_at: pull_request.updated_at,
                    why_listed: match pull_request.why_listed {
                        RememberedWhy::Mine => WhyListed::Mine,
                        RememberedWhy::ReviewRequested => WhyListed::ReviewRequested,
                        RememberedWhy::Involved => WhyListed::Involved,
                    },
                })
            })
            .collect()
    }

    pub(super) fn save(&self, pull_requests: &[RelevantPullRequest]) -> anyhow::Result<()> {
        let remembered: Vec<Remembered> = pull_requests
            .iter()
            .map(|pull_request| Remembered {
                url: pull_request.url.to_string(),
                title: pull_request.title.clone(),
                author: pull_request.author.clone(),
                is_draft: pull_request.is_draft,
                updated_at: pull_request.updated_at.clone(),
                why_listed: match pull_request.why_listed {
                    WhyListed::Mine => RememberedWhy::Mine,
                    WhyListed::ReviewRequested => RememberedWhy::ReviewRequested,
                    WhyListed::Involved => RememberedWhy::Involved,
                },
            })
            .collect();
        let folder = self
            .path
            .parent()
            .with_context(|| format!("{} has no folder", self.path.display()))?;
        std::fs::create_dir_all(folder)
            .with_context(|| format!("could not create {}", folder.display()))?;
        let temporary = self.path.with_extension("json.tmp");
        std::fs::write(&temporary, serde_json::to_string_pretty(&remembered)?)
            .with_context(|| format!("could not write {}", temporary.display()))?;
        std::fs::rename(&temporary, &self.path)
            .with_context(|| format!("could not replace {}", self.path.display()))
    }
}
