use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::Deserialize;

use crate::home::expand_home;
use crate::ide::IdeTemplate;
use crate::pull_request::RepoName;
use crate::summary_instructions::{SummaryInstructions, SummaryInstructionsChoice};
use crate::viewer::{KeyChanges, KeyProblem, Keymap, ResolvedKeymap, WrittenKeys};

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
    /// `wrap_lines = "l"` or `next_screen = ["right", "j"]`, by `Command::config_name`.
    #[serde(default)]
    keys: BTreeMap<String, toml::Value>,
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

/// The `ask_model` the reviewer picks in Settings: no key (the app's default), a Claude alias, or any model name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AskModelChoice {
    AppDefault,
    Opus,
    Sonnet,
    Haiku,
    Custom(String),
}

impl AskModelChoice {
    pub const PICKABLE: [Self; 3] = [Self::Opus, Self::Sonnet, Self::Haiku];

    pub fn custom_text(&self) -> Option<&str> {
        match self {
            Self::Custom(text) => Some(text),
            _ => None,
        }
    }

    /// What an alias runs `claude` with, so the reviewer sees what it means.
    pub fn shown_command(&self) -> Option<String> {
        Self::PICKABLE
            .contains(self)
            .then(|| format!("claude --model {}", self.config_text().unwrap_or_default()))
    }

    pub fn of(config_text: Option<&str>) -> Self {
        match config_text {
            None => Self::AppDefault,
            Some("opus") => Self::Opus,
            Some("sonnet") => Self::Sonnet,
            Some("haiku") => Self::Haiku,
            Some(other) => Self::Custom(other.to_owned()),
        }
    }

    /// What `ask_model` holds in the config, none for the app's default.
    pub fn config_text(&self) -> Option<&str> {
        match self {
            Self::AppDefault => None,
            Self::Opus => Some("opus"),
            Self::Sonnet => Some("sonnet"),
            Self::Haiku => Some("haiku"),
            Self::Custom(text) => Some(text),
        }
    }
}

/// The `ask_effort` the reviewer picks in Settings: no key (the app's default) or one `claude --effort` accepts.
/// A config can hold anything, so `Unknown` keeps what it says until the reviewer picks another.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AskEffortChoice {
    AppDefault,
    Low,
    Medium,
    High,
    XHigh,
    Max,
    Unknown(String),
}

impl AskEffortChoice {
    pub const PICKABLE: [Self; 5] = [Self::Low, Self::Medium, Self::High, Self::XHigh, Self::Max];

    pub fn of(config_text: Option<&str>) -> Self {
        match config_text {
            None => Self::AppDefault,
            Some("low") => Self::Low,
            Some("medium") => Self::Medium,
            Some("high") => Self::High,
            Some("xhigh") => Self::XHigh,
            Some("max") => Self::Max,
            Some(other) => Self::Unknown(other.to_owned()),
        }
    }

    pub fn config_text(&self) -> Option<&str> {
        match self {
            Self::AppDefault => None,
            Self::Low => Some("low"),
            Self::Medium => Some("medium"),
            Self::High => Some("high"),
            Self::XHigh => Some("xhigh"),
            Self::Max => Some("max"),
            Self::Unknown(text) => Some(text),
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

    pub fn keymap(&self) -> Result<Keymap, Vec<KeyProblem>> {
        self.key_changes().keymap()
    }

    pub fn resolved_keymap(&self) -> ResolvedKeymap {
        self.key_changes().resolve()
    }

    pub fn key_changes(&self) -> KeyChanges {
        KeyChanges::new(
            self.keys
                .iter()
                .map(|(name, value)| (name.clone(), written_keys(value))),
        )
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

fn written_keys(value: &toml::Value) -> WrittenKeys {
    let texts = match value {
        toml::Value::String(text) => Some(vec![text.clone()]),
        toml::Value::Array(items) => items
            .iter()
            .map(|item| item.as_str().map(str::to_owned))
            .collect(),
        _ => None,
    };
    texts.map_or_else(
        || WrittenKeys::NotText(value.to_string()),
        WrittenKeys::Texts,
    )
}

fn expand_env_home(path: &Path) -> PathBuf {
    std::env::home_dir().map_or_else(|| PathBuf::from(path), |home| expand_home(path, &home))
}
