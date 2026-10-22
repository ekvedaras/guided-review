use std::path::{Path, PathBuf};

use anyhow::Context;
use serde_json::{Map, Value};

use crate::pull_request::PullRequestUrl;
use crate::viewer::{Ask, AskStore};

/// Keeps Asks in the `asks` section of the per-PR state file
/// (`~/.guided-review/state/<owner>/<repo>/<pr>.json`), the same file as the read ticks.
/// It touches that section only and writes every other section back as it found it.
///
/// A stand-in until the state store owns an `asks` section itself; then only this adapter goes.
pub struct FileAskStore {
    path: PathBuf,
}

impl FileAskStore {
    pub fn new(home: &Path, pr: &PullRequestUrl) -> Self {
        Self {
            path: home
                .join(".guided-review/state")
                .join(pr.repo.to_string())
                .join(format!("{}.json", pr.number)),
        }
    }

    fn read_sections(&self) -> anyhow::Result<Map<String, Value>> {
        match std::fs::read_to_string(&self.path) {
            Ok(text) => serde_json::from_str(&text).with_context(|| {
                format!("{} is not a valid review state file", self.path.display())
            }),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Map::new()),
            Err(error) => {
                Err(error).with_context(|| format!("could not read {}", self.path.display()))
            }
        }
    }
}

impl AskStore for FileAskStore {
    fn load(&self) -> anyhow::Result<Vec<Ask>> {
        match self.read_sections()?.remove("asks") {
            Some(asks) => serde_json::from_value(asks).context("the asks section is not valid"),
            None => Ok(Vec::new()),
        }
    }

    fn save(&self, asks: &[Ask]) -> anyhow::Result<()> {
        let mut sections = self.read_sections()?;
        sections.insert("asks".to_string(), serde_json::to_value(asks)?);
        let folder = self.path.parent().expect("the state file sits in a folder");
        std::fs::create_dir_all(folder)
            .with_context(|| format!("could not create {}", folder.display()))?;
        let temp = self.path.with_extension("json.tmp");
        std::fs::write(&temp, serde_json::to_string_pretty(&sections)? + "\n")
            .with_context(|| format!("could not write {}", temp.display()))?;
        std::fs::rename(&temp, &self.path)
            .with_context(|| format!("could not write {}", self.path.display()))
    }
}
