use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::{Deserialize, Serialize};

/// What the app keeps across a restart that belongs to no one PR: the tours being written and the PRs
/// with an agent reply the reviewer has not seen.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct AppState {
    #[serde(default)]
    pub writings: Vec<String>,
    #[serde(default)]
    pub unseen_replies: Vec<String>,
}

/// `~/.guided-review/state/app.json`, beside the per-PR state files (which sit under owner folders).
#[derive(Clone)]
pub struct AppStateFile {
    path: PathBuf,
}

impl AppStateFile {
    pub fn in_home(home: &Path) -> Self {
        Self {
            path: home.join(".guided-review/state/app.json"),
        }
    }

    /// No file yet is an empty state. A file that cannot be parsed is moved to `app.broken.json` so the
    /// next save does not overwrite it.
    pub(super) fn load(&self) -> anyhow::Result<AppState> {
        let text = match std::fs::read_to_string(&self.path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(AppState::default());
            }
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("could not read {}", self.path.display()));
            }
        };
        serde_json::from_str(&text).or_else(|not_valid| {
            let backup = self.path.with_file_name("app.broken.json");
            std::fs::rename(&self.path, &backup).with_context(|| {
                format!(
                    "{} is not valid ({not_valid}) and could not be moved aside",
                    self.path.display()
                )
            })?;
            anyhow::bail!(
                "{} is not valid ({not_valid}); moved to {}",
                self.path.display(),
                backup.display()
            )
        })
    }

    pub(super) fn save(&self, state: &AppState) -> anyhow::Result<()> {
        let folder = self
            .path
            .parent()
            .with_context(|| format!("{} has no folder", self.path.display()))?;
        std::fs::create_dir_all(folder)
            .with_context(|| format!("could not create {}", folder.display()))?;
        let temporary = self.path.with_extension("json.tmp");
        std::fs::write(&temporary, serde_json::to_string_pretty(state)?)
            .with_context(|| format!("could not write {}", temporary.display()))?;
        std::fs::rename(&temporary, &self.path)
            .with_context(|| format!("could not replace {}", self.path.display()))
    }
}
