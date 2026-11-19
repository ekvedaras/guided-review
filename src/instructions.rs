use std::fmt;
use std::marker::PhantomData;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Deserializer};

use crate::app_folders::AppFolders;
use crate::config::Config;
use crate::home::expand_home;

/// Which agent a setting of instructions is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstructionsKind {
    /// `summary_instructions`: how the tour writer writes the reviewer summary.
    Summary,
    /// `review_instructions`: what the review agent looks for.
    Review,
}

impl InstructionsKind {
    pub const ALL: [Self; 2] = [Self::Summary, Self::Review];

    /// The key in the config.
    pub fn key(self) -> &'static str {
        match self {
            Self::Summary => "summary_instructions",
            Self::Review => "review_instructions",
        }
    }

    /// What the instructions are called to a reader.
    pub fn label(self) -> &'static str {
        match self {
            Self::Summary => "summary",
            Self::Review => "review",
        }
    }

    pub fn default_file(self, folders: &AppFolders) -> PathBuf {
        match self {
            Self::Summary => folders.summary_file(),
            Self::Review => folders.review_file(),
        }
    }

    pub fn off_env(self) -> &'static str {
        match self {
            Self::Summary => Config::SUMMARY_INSTRUCTIONS_OFF_ENV,
            Self::Review => Config::REVIEW_INSTRUCTIONS_OFF_ENV,
        }
    }

    /// What stands in when the config says nothing and the default file is not there.
    fn built_in(self) -> Option<&'static str> {
        match self {
            Self::Summary => None,
            Self::Review => Some(include_str!("review_guidance.md")),
        }
    }
}

/// Ties `Instructions` to the agent it is for, so a summary cannot be handed to the review agent.
pub trait Agent {
    const KIND: InstructionsKind;
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct TourWriter;

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Reviewer;

impl Agent for TourWriter {
    const KIND: InstructionsKind = InstructionsKind::Summary;
}

impl Agent for Reviewer {
    const KIND: InstructionsKind = InstructionsKind::Review;
}

/// What tells an agent how to do its part of the review, as far as it could be read.
#[derive(Debug, PartialEq, Eq)]
pub struct Instructions<A: Agent> {
    text: Option<String>,
    skill: Option<String>,
    warning: Option<String>,
    agent: PhantomData<A>,
}

/// What tells the tour writer how to write the reviewer summary.
pub type SummaryInstructions = Instructions<TourWriter>;

/// What tells the review agent what to look for.
pub type ReviewInstructions = Instructions<Reviewer>;

/// How `summary_instructions` or `review_instructions` is set in the config, before it is resolved to the instructions themselves.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum InstructionsChoice {
    /// No key: the default file in the config folder (`summary.md`, `review.md`) when it exists.
    #[default]
    DefaultFile,
    /// A path as written, which may start with `~/`.
    OtherFile(PathBuf),
    Skill(SkillName),
    Off,
    /// What the config holds when it is none of the above, as written.
    Unreadable(String),
}

impl InstructionsChoice {
    pub fn skill_text(&self) -> Option<&str> {
        match self {
            Self::Skill(name) => Some(name.as_str()),
            _ => None,
        }
    }

    pub const OFF: &'static str = "off";
    pub const SKILL_KEY: &'static str = "skill";

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

    pub fn resolve<A: Agent>(&self, folders: &AppFolders) -> Instructions<A> {
        match self {
            Self::Off => Instructions::none(),
            Self::OtherFile(path) => read_file(&expand_home(path, folders.home())),
            Self::Skill(name) => Instructions::of_skill(name),
            Self::Unreadable(written) => Instructions::unreadable(written),
            Self::DefaultFile => {
                let default = A::KIND.default_file(folders);
                if default.is_file() {
                    read_file(&default)
                } else {
                    Instructions::built_in()
                }
            }
        }
    }
}

impl<'de> Deserialize<'de> for InstructionsChoice {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        toml::Value::deserialize(deserializer).map(|value| Self::from_value(&value))
    }
}

fn read_file<A: Agent>(path: &Path) -> Instructions<A> {
    Instructions::from_file(path, std::fs::read_to_string(path))
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

impl<A: Agent> Instructions<A> {
    pub fn none() -> Self {
        Self {
            text: None,
            skill: None,
            warning: None,
            agent: PhantomData,
        }
    }

    fn built_in() -> Self {
        Self {
            text: A::KIND.built_in().map(str::to_owned),
            ..Self::none()
        }
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
                "{} {written} is not a file path, \"{}\" or {{ {} = \"<name>\" }}",
                A::KIND.key(),
                InstructionsChoice::OFF,
                InstructionsChoice::SKILL_KEY
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
                "{} instructions file {} could not be read: {error}",
                A::KIND.label(),
                path.display()
            )),
            ..Self::none()
        }
    }

    /// What a launch of the tour agent goes by: a config that cannot be read is no instructions and a warning.
    pub fn of(folders: &AppFolders) -> Self {
        match Config::load_from(folders) {
            Ok(config) => config.instructions(folders),
            Err(error) => Self {
                warning: Some(format!(
                    "{} instructions not read: {error:#}",
                    A::KIND.label()
                )),
                ..Self::none()
            },
        }
    }

    /// The skill a launch of the tour agent runs for the reviewer summary, after saying what could not be read.
    pub fn skill_for_launch(folders: &AppFolders) -> Option<String> {
        let instructions = Self::of(folders);
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

impl<A: Agent> fmt::Display for Instructions<A> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(warning) = &self.warning {
            writeln!(f, "\nwarning: {warning}")?;
        }
        if let Some(skill) = &self.skill {
            writeln!(
                f,
                "\n{} instructions: run the skill {skill}",
                A::KIND.label()
            )?;
        }
        if let Some(text) = &self.text {
            write!(f, "\n{} instructions:\n{text}", A::KIND.label())?;
            if !text.ends_with('\n') {
                writeln!(f)?;
            }
        }
        Ok(())
    }
}
