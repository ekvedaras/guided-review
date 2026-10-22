use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::{Deserialize, Serialize};

use crate::pull_request::{PullRequestUrl, Sha};

use super::review::StepId;

/// What the viewer remembers about one PR, apart from the tour: everything that is the reviewer's,
/// not the tour writer's. Every section defaults to empty, so a file written before a section
/// existed still loads.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct ReviewState {
    #[serde(default)]
    read_ticks: BTreeMap<StepId, ReadTick>,
    /// Sections this build does not know, written by a newer one. Kept so saving does not drop them.
    #[serde(flatten)]
    unknown_sections: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadTick {
    /// The head the step was read at; after new commits it tells what changed since.
    pub head: String,
}

impl ReviewState {
    pub fn tick(&mut self, step: &StepId, head: &Sha) {
        self.read_ticks.insert(
            step.clone(),
            ReadTick {
                head: head.to_string(),
            },
        );
    }

    pub fn untick(&mut self, step: &StepId) {
        self.read_ticks.remove(step);
    }

    pub fn read_tick(&self, step: &StepId) -> Option<&ReadTick> {
        self.read_ticks.get(step)
    }
}

/// `~/.guided-review/state/<owner>/<repo>/<pr>.json`
pub struct ReviewStateFile {
    path: PathBuf,
}

impl ReviewStateFile {
    pub fn for_pr(home: &Path, pr: &PullRequestUrl) -> Self {
        Self {
            path: home
                .join(".guided-review/state")
                .join(pr.repo.to_string())
                .join(format!("{}.json", pr.number)),
        }
    }

    /// A PR not reviewed before has no file yet: that is an empty state, not an error.
    pub fn load(&self) -> anyhow::Result<ReviewState> {
        match std::fs::read_to_string(&self.path) {
            Ok(text) => serde_json::from_str(&text).with_context(|| {
                format!("{} is not a valid review state file", self.path.display())
            }),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Ok(ReviewState::default())
            }
            Err(error) => {
                Err(error).with_context(|| format!("could not read {}", self.path.display()))
            }
        }
    }

    /// Written to a temp file first, so a crash mid-write never leaves half a file behind.
    pub fn save(&self, state: &ReviewState) -> anyhow::Result<()> {
        let folder = self.path.parent().expect("the state file sits in a folder");
        std::fs::create_dir_all(folder)
            .with_context(|| format!("could not create {}", folder.display()))?;
        let temp = self.path.with_extension("json.tmp");
        let text = serde_json::to_string_pretty(state)?;
        std::fs::write(&temp, text + "\n")
            .with_context(|| format!("could not write {}", temp.display()))?;
        std::fs::rename(&temp, &self.path)
            .with_context(|| format!("could not write {}", self.path.display()))
    }
}
