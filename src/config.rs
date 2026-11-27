use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::Deserialize;

use crate::app_folders::AppFolders;
use crate::codebase::Codebase;
use crate::home::expand_home;
use crate::ide::IdeTemplate;
use crate::instructions::{Agent, Instructions, InstructionsChoice, InstructionsKind};
use crate::keys::{KeyChanges, KeyProblem, Keymap, ResolvedKeymap, SendChoice, WrittenKeys};
use crate::merge_method::MergeMethod;
use crate::more_lines::MoreLines;
use crate::navigation::ServerAnswer;
use crate::pull_request::HostedRepo;

/// `config.toml` in the config folder.
#[derive(Default, Deserialize)]
pub struct Config {
    /// `"owner/repo" = "~/path/to/clone"`, a repo on GitHub; `"<host>:<repo>"` names its host.
    #[serde(default)]
    clones: BTreeMap<String, String>,
    /// `idea://open?file={path}&line={line}` or a shell command such as `code --goto {path}:{line}`.
    #[serde(default)]
    ide: Option<String>,
    #[serde(default)]
    summary_instructions: InstructionsChoice,
    #[serde(default)]
    review_instructions: InstructionsChoice,
    /// `wrap_lines = "l"` or `next_screen = ["right", "j"]`, by `Command::config_name`.
    #[serde(default)]
    keys: BTreeMap<String, toml::Value>,
    /// How many lines one "more lines" step shows. Any value is read, so a wrong one cannot stop the app opening.
    #[serde(default)]
    more_lines: Option<toml::Value>,
    /// `"merge"`, `"squash"` or `"rebase"`: how auto-merge and Merge now merge.
    #[serde(default)]
    merge_method: Option<String>,
    /// `"owner/repo" = "squash"`: a repo's own merge method, over `merge_method`. Keyed as `clones` is.
    #[serde(default)]
    merge_methods: BTreeMap<String, String>,
    /// `"owner/repo" = true`: whether language servers answer the Definition card in that repo. A
    /// repo that is not in it has not been asked.
    #[serde(default)]
    language_servers: BTreeMap<String, toml::Value>,
    /// A send action (`"add_to_review"`, `"comment_now"`, `"suggest_change"`, `"ask_agent"` or
    /// `"ask_agent_now"`): the one with Cmd+Enter and the accent colour.
    #[serde(default)]
    primary_comment_action: Option<String>,
    /// The send action with Cmd+Shift+Enter, one other than the primary.
    #[serde(default)]
    secondary_comment_action: Option<String>,
}

impl Config {
    pub fn from_text(text: &str) -> anyhow::Result<Self> {
        Self::parse(text, "the config")
    }

    fn parse(text: &str, source: &str) -> anyhow::Result<Self> {
        toml::from_str(text).with_context(|| format!("could not read {source}"))
    }

    pub fn ide_template(&self) -> Option<IdeTemplate> {
        self.ide.clone().map(IdeTemplate::new)
    }

    pub fn load() -> anyhow::Result<Self> {
        Self::load_from(&AppFolders::running()?)
    }

    pub fn load_from(folders: &AppFolders) -> anyhow::Result<Self> {
        let path = folders.config_file();
        match std::fs::read_to_string(&path) {
            Ok(text) => Self::parse(&text, &path.display().to_string()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error).with_context(|| format!("could not read {}", path.display())),
        }
    }

    /// What the config says, or the default file (`summary.md`, `review.md`) in the config folder when it says nothing and the file exists.
    pub fn instructions<A: Agent>(&self, folders: &AppFolders) -> Instructions<A> {
        if std::env::var_os(A::KIND.off_env()).is_some() {
            return Instructions::none();
        }
        match A::KIND {
            InstructionsKind::Summary => &self.summary_instructions,
            InstructionsKind::Review => &self.review_instructions,
        }
        .resolve(folders)
    }

    /// What the config says, or twenty when it says nothing or something that is not a number of lines.
    pub fn more_lines(&self) -> MoreLines {
        self.more_lines
            .as_ref()
            .and_then(toml::Value::as_integer)
            .and_then(MoreLines::new)
            .unwrap_or_default()
    }

    /// The repo's own method, else the global one, else a merge commit. A value that names no
    /// method is passed over, so a typo cannot stop the app opening.
    pub fn merge_method(&self, repo: &HostedRepo) -> MergeMethod {
        let own = self
            .merge_methods
            .iter()
            .find(|(name, _)| name.parse::<HostedRepo>().is_ok_and(|name| name == *repo))
            .and_then(|(_, method)| MergeMethod::of_config_text(method));
        own.or_else(|| {
            self.merge_method
                .as_deref()
                .and_then(MergeMethod::of_config_text)
        })
        .unwrap_or_default()
    }

    /// What the reviewer answered for the codebase; none while it has not been asked. A value that is
    /// not true or false is passed over, so a typo cannot stop the app opening.
    pub fn language_servers(&self, codebase: &Codebase) -> Option<ServerAnswer> {
        self.language_servers
            .iter()
            .find(|(name, _)| name.parse::<Codebase>().is_ok_and(|key| key == *codebase))
            .and_then(|(_, answer)| answer.as_bool())
            .map(ServerAnswer::of)
    }

    pub fn send_choice(&self) -> SendChoice {
        SendChoice::of_config_text(
            self.primary_comment_action.as_deref(),
            self.secondary_comment_action.as_deref(),
        )
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
        .with_send_choice(self.send_choice())
    }

    /// Every clone Settings names, whatever its repo.
    pub fn clone_folders(&self) -> Vec<PathBuf> {
        self.clones
            .values()
            .map(|path| expand_env_home(Path::new(path)))
            .collect()
    }

    pub fn clone_of(&self, repo: &HostedRepo) -> Option<PathBuf> {
        self.clones
            .iter()
            .find(|(name, _)| name.parse::<HostedRepo>().is_ok_and(|name| name == *repo))
            .map(|(_, path)| expand_env_home(Path::new(path)))
    }
}

impl Config {
    /// What a launch of an agent goes by: a config that cannot be read is no instructions and a warning.
    pub fn read_instructions<A: Agent>(folders: &AppFolders) -> Instructions<A> {
        match Self::load_from(folders) {
            Ok(config) => config.instructions(folders),
            Err(error) => Instructions::config_not_read(&error),
        }
    }

    /// The skill a launch of an agent runs, after saying what could not be read.
    pub fn skill_for_launch<A: Agent>(folders: &AppFolders) -> Option<String> {
        let instructions = Self::read_instructions::<A>(folders);
        if let Some(warning) = instructions.warning() {
            eprintln!("warning: {warning}");
        }
        instructions.skill().map(str::to_owned)
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
