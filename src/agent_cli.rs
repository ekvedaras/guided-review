//! How an agent runs guided-review. Every message the app types into an agent, and the tool list of every
//! agent it starts, names the command as this says.

use std::fmt;
use std::path::PathBuf;

use crate::build::Build;
use crate::prompt_safety::shell_word;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum AgentCli {
    /// `guided-review` as the agent's `PATH` finds it, which is the release install's bin link.
    #[default]
    OnPath,
    /// The app's own binary by its full path. A terminal the reviewer picked has the release install first on
    /// `PATH`, so a bare `guided-review` there runs the release, not the trial build that sent the message.
    At(PathBuf),
}

impl AgentCli {
    pub fn of_running_app() -> Self {
        crate::platform::running_binary().map_or(Self::OnPath, |binary| {
            Self::for_build(crate::update::running_build(), binary)
        })
    }

    pub fn for_build(build: Build, binary: PathBuf) -> Self {
        match build {
            Build::Release => Self::OnPath,
            Build::Local | Build::Test => Self::At(binary),
        }
    }

    /// The permission rules that let an agent run `guided-review <command>`: by name, as the tour skill
    /// writes it, and by this binary's path, as the app's messages write it.
    pub fn bash_rules(&self, command: &str) -> Vec<String> {
        let by_name = format!("Bash(guided-review {command}:*)");
        match self {
            Self::OnPath => vec![by_name],
            Self::At(_) => vec![by_name, format!("Bash({self} {command}:*)")],
        }
    }

    /// What a task that runs the tour skill adds, since the skill names its commands by name.
    pub fn skill_note(&self) -> Option<String> {
        match self {
            Self::OnPath => None,
            Self::At(_) => Some(format!(
                "Run every `guided-review` command the skill names as `{self}`, the build of the app that asked."
            )),
        }
    }
}

impl fmt::Display for AgentCli {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OnPath => f.write_str("guided-review"),
            Self::At(binary) => f.write_str(&shell_word(&binary.display().to_string())),
        }
    }
}
