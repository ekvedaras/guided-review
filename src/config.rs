use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::Deserialize;

use crate::home::expand_home;
use crate::ide::IdeTemplate;
use crate::pull_request::RepoName;
use crate::summary_instructions::{SummaryInstructions, SummaryInstructionsChoice};

/// `~/.guided-review/config.toml`
#[derive(Default, Deserialize)]
pub struct Config {
    /// `"owner/repo" = "~/path/to/clone"`
    #[serde(default)]
    clones: BTreeMap<String, String>,
    #[serde(default)]
    ask_model: Option<String>,
    #[serde(default)]
    ask_effort: Option<String>,
    /// `idea://open?file={path}&line={line}` or a shell command such as `code --goto {path}:{line}`.
    #[serde(default)]
    ide: Option<String>,
    #[serde(default)]
    summary_instructions: SummaryInstructionsChoice,
}

/// Which model answers an Ask, and how hard it thinks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AskSettings {
    pub model: String,
    pub effort: String,
}

impl Default for AskSettings {
    fn default() -> Self {
        Self {
            model: "opus".to_owned(),
            effort: "medium".to_owned(),
        }
    }
}

impl AskSettings {
    pub fn from_config_text(text: &str) -> anyhow::Result<Self> {
        Ok(Config::from_text(text)?.ask_settings())
    }
}

impl Config {
    pub const SUMMARY_INSTRUCTIONS_OFF_ENV: &'static str = "GUIDED_REVIEW_SUMMARY_INSTRUCTIONS_OFF";

    pub fn from_text(text: &str) -> anyhow::Result<Self> {
        Self::parse(text, "the config")
    }

    fn parse(text: &str, source: &str) -> anyhow::Result<Self> {
        toml::from_str(text).with_context(|| format!("could not read {source}"))
    }

    pub fn ide_template(&self) -> Option<IdeTemplate> {
        self.ide.clone().map(IdeTemplate::new)
    }

    pub fn file_in(home: &Path) -> PathBuf {
        home.join(".guided-review/config.toml")
    }

    pub fn load() -> anyhow::Result<Self> {
        let Some(home) = std::env::home_dir() else {
            return Ok(Self::default());
        };
        Self::load_from(&home)
    }

    pub fn load_from(home: &Path) -> anyhow::Result<Self> {
        let path = Self::file_in(home);
        match std::fs::read_to_string(&path) {
            Ok(text) => Self::parse(&text, &path.display().to_string()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error).with_context(|| format!("could not read {}", path.display())),
        }
    }

    /// What the config says, or the default file `~/.guided-review/summary.md` when it says nothing and the file exists.
    pub fn summary_instructions(&self, home: &Path) -> SummaryInstructions {
        if std::env::var_os(Self::SUMMARY_INSTRUCTIONS_OFF_ENV).is_some() {
            return SummaryInstructions::none();
        }
        self.summary_instructions.resolve(home)
    }

    pub fn ask_settings(&self) -> AskSettings {
        let default = AskSettings::default();
        AskSettings {
            model: self.ask_model.clone().unwrap_or(default.model),
            effort: self.ask_effort.clone().unwrap_or(default.effort),
        }
    }

    pub fn clone_of(&self, repo: &RepoName) -> Option<PathBuf> {
        self.clones
            .iter()
            .find(|(name, _)| {
                name.split_once('/')
                    .is_some_and(|(owner, name)| RepoName::new(owner, name) == *repo)
            })
            .map(|(_, path)| expand_env_home(Path::new(path)))
    }
}

fn expand_env_home(path: &Path) -> PathBuf {
    std::env::home_dir().map_or_else(|| PathBuf::from(path), |home| expand_home(path, &home))
}
