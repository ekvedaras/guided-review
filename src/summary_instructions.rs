use std::fmt;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Deserializer};

use crate::home::expand_home;

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
    OtherFile(PathBuf),
    Skill(SkillName),
    Off,
    /// What the config holds when it is none of the above, as written.
    Unreadable(String),
}

impl SummaryInstructionsChoice {
    pub fn skill_text(&self) -> Option<&str> {
        match self {
            Self::Skill(name) => Some(name.as_str()),
            _ => None,
        }
    }

    pub const OFF: &'static str = "off";
    pub const SKILL_KEY: &'static str = "skill";
    const DEFAULT_FILE: &'static str = ".guided-review/summary.md";

    pub fn default_file_in(home: &Path) -> PathBuf {
        home.join(Self::DEFAULT_FILE)
    }

    /// A typed skill name is a choice only when outline would accept it.
    pub fn skill_named(name: &str) -> Self {
        SkillName::parse(name.trim()).map_or_else(
            |_| Self::Unreadable(format!("{{ {} = {name:?} }}", Self::SKILL_KEY)),
            Self::Skill,
        )
    }

    pub fn from_value(value: &toml::Value) -> Self {
        match value.as_str() {
            Some(Self::OFF) => Self::Off,
            Some(path) => Self::OtherFile(PathBuf::from(path)),
            None => match value.get(Self::SKILL_KEY).and_then(toml::Value::as_str) {
                Some(name) => Self::skill_named(name),
                None => Self::Unreadable(value.to_string().trim().to_owned()),
            },
        }
    }

    pub fn resolve(&self, home: &Path) -> SummaryInstructions {
        match self {
            Self::Off => SummaryInstructions::none(),
            Self::OtherFile(path) => read_file(&expand_home(path, home)),
            Self::Skill(name) => SummaryInstructions::of_skill(name),
            Self::Unreadable(written) => SummaryInstructions::unreadable(written),
            Self::DefaultFile => {
                let default = Self::default_file_in(home);
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

/// A skill the tour writer runs through the Skill tool, such as `warm-up` or `plugin:skill`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct SkillName(String);

#[derive(Debug, PartialEq, Eq)]
pub struct NotASkillName;

impl fmt::Display for NotASkillName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("not a skill name")
    }
}

impl std::error::Error for NotASkillName {}

impl SkillName {
    /// Whitespace, `,`, `(` and `)` would add more than one rule to the tour agent's allowed tools.
    pub fn parse(name: &str) -> Result<Self, NotASkillName> {
        let is_one_rule = !name.is_empty()
            && !name.contains(|c: char| c.is_whitespace() || matches!(c, ',' | '(' | ')'));
        is_one_rule
            .then(|| Self(name.to_owned()))
            .ok_or(NotASkillName)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
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
                "summary_instructions {written} is not a file path, \"{}\" or {{ {} = \"<name>\" }}",
                SummaryInstructionsChoice::OFF,
                SummaryInstructionsChoice::SKILL_KEY
            )),
            ..Self::none()
        }
    }

    fn of_skill(name: &SkillName) -> Self {
        Self {
            skill: Some(name.as_str().to_owned()),
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

    /// What a launch of the tour agent goes by: a config that cannot be read is no instructions and a warning.
    pub fn of_home(home: &Path) -> Self {
        match crate::config::Config::load_from(home) {
            Ok(config) => config.summary_instructions(home),
            Err(error) => Self {
                warning: Some(format!("summary instructions not read: {error:#}")),
                ..Self::none()
            },
        }
    }

    /// The skill a launch of the tour agent runs for the reviewer summary, after saying what could not be read.
    pub fn skill_for_launch(home: &Path) -> Option<String> {
        let instructions = Self::of_home(home);
        if let Some(warning) = instructions.warning() {
            eprintln!("warning: {warning}");
        }
        instructions.skill().map(str::to_owned)
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
