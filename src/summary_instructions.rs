use std::fmt;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Deserializer};

pub const DEFAULT_FILE: &str = ".guided-review/summary.md";

/// What tells the tour writer how to write the reviewer summary, as far as it could be read.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct SummaryInstructions {
    text: Option<String>,
    skill: Option<String>,
    warning: Option<String>,
}

/// How `summary_instructions` is set in the config, before it is resolved to the instructions themselves.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum SummaryInstructionsChoice {
    /// No key: `~/.guided-review/summary.md`, when it exists.
    #[default]
    DefaultFile,
    /// A path as written, which may start with `~/`.
    OtherFile(String),
    Off,
    /// A skill name as written.
    Skill(String),
    /// What the config holds when it is none of the above, as written.
    Unreadable(String),
}

impl SummaryInstructionsChoice {
    pub const OFF: &'static str = "off";

    pub fn from_value(value: &toml::Value) -> Self {
        match value.as_str() {
            Some(Self::OFF) => Self::Off,
            Some(path) => Self::OtherFile(path.to_owned()),
            None => match value.get("skill").and_then(toml::Value::as_str) {
                Some(name) => Self::Skill(name.to_owned()),
                None => Self::Unreadable(value.to_string().trim().to_owned()),
            },
        }
    }

    pub fn resolve(&self, home: &Path) -> SummaryInstructions {
        match self {
            Self::Off => SummaryInstructions::none(),
            Self::OtherFile(path) => read_file(&expand_home(path, home)),
            Self::Skill(name) => SummaryInstructions::from_skill(name),
            Self::Unreadable(written) => SummaryInstructions::unreadable(written),
            Self::DefaultFile => {
                let default = home.join(DEFAULT_FILE);
                if default.is_file() {
                    read_file(&default)
                } else {
                    SummaryInstructions::none()
                }
            }
        }
    }
}

impl<'de> Deserialize<'de> for SummaryInstructionsChoice {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        toml::Value::deserialize(deserializer).map(|value| Self::from_value(&value))
    }
}

fn read_file(path: &Path) -> SummaryInstructions {
    SummaryInstructions::from_file(path, std::fs::read_to_string(path))
}

/// `~/` is the given home, so a config means the same wherever it is read.
pub fn expand_home(path: &str, home: &Path) -> PathBuf {
    path.strip_prefix("~/")
        .map_or_else(|| PathBuf::from(path), |rest| home.join(rest))
}

impl SummaryInstructions {
    pub fn none() -> Self {
        Self::default()
    }

    /// Blank text is no instructions: a file that was created and never filled in asks for nothing.
    pub fn from_file(path: &Path, read: std::io::Result<String>) -> Self {
        match read {
            Ok(text) if text.trim().is_empty() => Self::none(),
            Ok(text) => Self {
                text: Some(text),
                ..Self::none()
            },
            Err(error) => Self::broken(path, &error),
        }
    }

    fn unreadable(written: &str) -> Self {
        Self {
            warning: Some(format!(
                "summary_instructions {written} is not a file path, \"{}\" or {{ skill = \"<name>\" }}",
                SummaryInstructionsChoice::OFF
            )),
            ..Self::none()
        }
    }

    /// A name that would add more than one rule to the tour agent's allowed tools is a warning, not a skill.
    pub fn from_skill(name: &str) -> Self {
        let name = name.trim();
        if name.is_empty() {
            return Self::none();
        }
        if name.contains(|c: char| c.is_whitespace() || matches!(c, ',' | '(' | ')')) {
            return Self {
                warning: Some(format!(
                    "summary instructions skill {name:?} is not a skill name"
                )),
                ..Self::none()
            };
        }
        Self {
            skill: Some(name.to_owned()),
            ..Self::none()
        }
    }

    fn broken(path: &Path, error: &std::io::Error) -> Self {
        Self {
            warning: Some(format!(
                "summary instructions file {} could not be read: {error}",
                path.display()
            )),
            ..Self::none()
        }
    }

    pub fn text(&self) -> Option<&str> {
        self.text.as_deref()
    }

    /// The skill the tour writer runs for the reviewer summary.
    pub fn skill(&self) -> Option<&str> {
        self.skill.as_deref()
    }

    pub fn warning(&self) -> Option<&str> {
        self.warning.as_deref()
    }

    pub fn are_set(&self) -> bool {
        self.text.is_some() || self.skill.is_some()
    }
}

impl fmt::Display for SummaryInstructions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(warning) = &self.warning {
            writeln!(f, "\nwarning: {warning}")?;
        }
        if let Some(skill) = &self.skill {
            writeln!(f, "\nsummary instructions: run the skill {skill}")?;
        }
        if let Some(text) = &self.text {
            write!(f, "\nsummary instructions:\n{text}")?;
            if !text.ends_with('\n') {
                writeln!(f)?;
            }
        }
        Ok(())
    }
}
