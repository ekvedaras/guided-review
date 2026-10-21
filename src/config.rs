use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::Context;
use serde::Deserialize;

use crate::pull_request::RepoName;

/// `~/.guided-review/config.toml`
#[derive(Default, Deserialize)]
pub struct Config {
    /// `"owner/repo" = "~/path/to/clone"`
    #[serde(default)]
    clones: BTreeMap<String, String>,
}

impl Config {
    pub fn load() -> anyhow::Result<Self> {
        let Some(home) = std::env::home_dir() else {
            return Ok(Self::default());
        };
        let path = home.join(".guided-review/config.toml");
        match std::fs::read_to_string(&path) {
            Ok(text) => {
                toml::from_str(&text).with_context(|| format!("could not read {}", path.display()))
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error).with_context(|| format!("could not read {}", path.display())),
        }
    }

    pub fn clone_of(&self, repo: &RepoName) -> Option<PathBuf> {
        self.clones
            .iter()
            .find(|(name, _)| {
                name.split_once('/')
                    .is_some_and(|(owner, name)| RepoName::new(owner, name) == *repo)
            })
            .map(|(_, path)| expand_home(path))
    }
}

fn expand_home(path: &str) -> PathBuf {
    match (path.strip_prefix("~/"), std::env::home_dir()) {
        (Some(rest), Some(home)) => home.join(rest),
        _ => PathBuf::from(path),
    }
}
