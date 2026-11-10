use std::path::{Path, PathBuf};

use anyhow::Context;
use toml_edit::{DocumentMut, InlineTable, Item, Table, TableLike, Value};

use crate::ide::IdeTemplate;
use crate::local_git::{CloneProblem, LocalGit};
use crate::pull_request::RepoName;

/// The reviewer's settings in `~/.guided-review/config.toml`, as the Settings panel shows and edits them.
/// Save writes only the keys that changed, so the reviewer's comments and key order survive.
pub struct Settings {
    home: PathBuf,
    file: PathBuf,
    document: DocumentMut,
    saved: Values,
    edited: Values,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Values {
    ask_model: Option<String>,
    ask_effort: Option<String>,
    ide: Option<IdeTemplate>,
    summary_instructions: SummaryInstructionsChoice,
    clones: Vec<CloneRow>,
}

impl Values {
    fn read(document: &DocumentMut) -> anyhow::Result<Self> {
        let text = |key: &str| document.get(key).and_then(Item::as_str).map(str::to_owned);
        Ok(Self {
            ask_model: text(ASK_MODEL),
            ask_effort: text(ASK_EFFORT),
            ide: text(IDE).map(IdeTemplate::new),
            summary_instructions: SummaryInstructionsChoice::read(
                document.get(SUMMARY_INSTRUCTIONS),
            )?,
            clones: document
                .get(CLONES)
                .and_then(Item::as_table_like)
                .into_iter()
                .flat_map(TableLike::iter)
                .filter_map(|(repo, folder)| {
                    Some(CloneRow::new(repo, PathBuf::from(folder.as_str()?)))
                })
                .collect(),
        })
    }
}

/// One `[clones]` entry: the repo as typed, and the folder of its local clone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloneRow {
    pub repo: String,
    pub folder: PathBuf,
}

impl CloneRow {
    pub fn new(repo: &str, folder: PathBuf) -> Self {
        Self {
            repo: repo.to_owned(),
            folder,
        }
    }

    fn repo_name(&self) -> Option<RepoName> {
        let (owner, name) = self.repo.split_once('/')?;
        let well_formed = !owner.is_empty() && !name.is_empty() && !name.contains('/');
        well_formed.then(|| RepoName::new(owner, name))
    }
}

/// A setting that would be wrong if saved, so Save writes nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingProblem {
    Clone {
        row: usize,
        problem: CloneRowProblem,
    },
    SummaryFileGone(PathBuf),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloneRowProblem {
    NotOwnerSlashRepo,
    SameRepoTwice,
    FolderGone,
    NoRemoteOfRepo,
}

impl From<CloneProblem> for CloneRowProblem {
    fn from(problem: CloneProblem) -> Self {
        match problem {
            CloneProblem::FolderGone => Self::FolderGone,
            CloneProblem::NoRemoteOfRepo => Self::NoRemoteOfRepo,
        }
    }
}

const ASK_MODEL: &str = "ask_model";
const ASK_EFFORT: &str = "ask_effort";
const IDE: &str = "ide";
const CLONES: &str = "clones";
const SUMMARY_INSTRUCTIONS: &str = "summary_instructions";
const SKILL: &str = "skill";
const OFF: &str = "off";
const DEFAULT_SUMMARY_FILE: &str = ".guided-review/summary.md";
const SKILLS_FOLDER: &str = ".claude/skills";

/// How `summary_instructions` is set in the config, before it is resolved to the instructions themselves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SummaryInstructionsChoice {
    /// No key: `~/.guided-review/summary.md`, when it exists.
    DefaultFile,
    OtherFile(PathBuf),
    Skill(SkillName),
    Off,
}

impl SummaryInstructionsChoice {
    fn read(item: Option<&Item>) -> anyhow::Result<Self> {
        let Some(item) = item else {
            return Ok(Self::DefaultFile);
        };
        if let Some(text) = item.as_str() {
            return Ok(if text == OFF {
                Self::Off
            } else {
                Self::OtherFile(PathBuf::from(text))
            });
        }
        item.get(SKILL)
            .and_then(Item::as_str)
            .map(|name| Self::Skill(SkillName::new(name)))
            .with_context(|| {
                format!(
                    "{SUMMARY_INSTRUCTIONS} is not a path, \"{OFF}\" or {{ {SKILL} = \"<name>\" }}"
                )
            })
    }

    fn to_value(&self) -> anyhow::Result<Option<Value>> {
        Ok(match self {
            Self::DefaultFile => None,
            Self::Off => Some(Value::from(OFF)),
            Self::OtherFile(path) => Some(Value::from(config_path(path)?)),
            Self::Skill(name) => {
                let mut table = InlineTable::new();
                table.insert(SKILL, Value::from(name.as_str()));
                Some(Value::InlineTable(table))
            }
        })
    }
}

/// A skill the tour writer runs through the Skill tool, such as `warm-up` or `plugin:skill`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct SkillName(String);

impl SkillName {
    pub fn new(name: &str) -> Self {
        Self(name.to_owned())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum SaveOutcome {
    NothingChanged,
    /// The config file was written; the open PR is opened again so the new settings apply.
    ReopenOpenPr,
    /// Nothing was written; each problem goes under its setting.
    Refused(Vec<SettingProblem>),
}

impl Settings {
    pub fn load(home: &Path) -> anyhow::Result<Self> {
        let file = home.join(".guided-review/config.toml");
        let text = match std::fs::read_to_string(&file) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(error) => {
                return Err(error).with_context(|| format!("could not read {}", file.display()));
            }
        };
        let document = text
            .parse::<DocumentMut>()
            .with_context(|| format!("could not read {}", file.display()))?;
        let saved = Values::read(&document)
            .with_context(|| format!("could not read {}", file.display()))?;
        Ok(Self {
            home: home.to_path_buf(),
            file,
            document,
            edited: saved.clone(),
            saved,
        })
    }

    pub fn ask_model(&self) -> Option<&str> {
        self.edited.ask_model.as_deref()
    }

    pub fn set_ask_model(&mut self, model: Option<String>) {
        self.edited.ask_model = model;
    }

    pub fn ask_effort(&self) -> Option<&str> {
        self.edited.ask_effort.as_deref()
    }

    pub fn set_ask_effort(&mut self, effort: Option<String>) {
        self.edited.ask_effort = effort;
    }

    pub fn ide(&self) -> Option<&IdeTemplate> {
        self.edited.ide.as_ref()
    }

    pub fn set_ide(&mut self, ide: Option<IdeTemplate>) {
        self.edited.ide = ide;
    }

    pub fn summary_instructions(&self) -> &SummaryInstructionsChoice {
        &self.edited.summary_instructions
    }

    pub fn set_summary_instructions(&mut self, choice: SummaryInstructionsChoice) {
        self.edited.summary_instructions = choice;
    }

    pub fn default_file(&self) -> PathBuf {
        self.home.join(DEFAULT_SUMMARY_FILE)
    }

    pub fn default_file_exists(&self) -> bool {
        self.default_file().is_file()
    }

    pub fn clones(&self) -> &[CloneRow] {
        &self.edited.clones
    }

    /// Returns the new row.
    pub fn add_clone(&mut self) -> usize {
        self.edited.clones.push(CloneRow::new("", PathBuf::new()));
        self.edited.clones.len() - 1
    }

    pub fn remove_clone(&mut self, row: usize) {
        if row < self.edited.clones.len() {
            self.edited.clones.remove(row);
        }
    }

    pub fn set_clone_repo(&mut self, row: usize, repo: String) {
        if let Some(clone) = self.edited.clones.get_mut(row) {
            clone.repo = repo;
        }
    }

    pub fn set_clone_folder(&mut self, row: usize, folder: PathBuf) {
        if let Some(clone) = self.edited.clones.get_mut(row) {
            clone.folder = folder;
        }
    }

    /// A folder picked in the folder picker also fills the repo, from the folder's `origin` remote.
    pub fn pick_clone_folder(&mut self, row: usize, folder: PathBuf) {
        let origin = LocalGit::origin_of(&self.in_home(&folder));
        if let (Some(clone), Some(origin)) = (self.edited.clones.get_mut(row), origin) {
            clone.repo = origin.to_string();
        }
        self.set_clone_folder(row, folder);
    }

    /// The skills in `~/.claude/skills`, symlinked ones too, by name.
    pub fn skills(&self) -> Vec<SkillName> {
        let Ok(entries) = std::fs::read_dir(self.home.join(SKILLS_FOLDER)) else {
            return Vec::new();
        };
        let mut skills: Vec<SkillName> = entries
            .filter_map(Result::ok)
            .filter(|entry| entry.path().join("SKILL.md").is_file())
            .filter_map(|entry| entry.file_name().to_str().map(SkillName::new))
            .collect();
        skills.sort();
        skills
    }

    /// One problem per setting that would be wrong if saved.
    pub fn check(&self) -> Vec<SettingProblem> {
        let clones = &self.edited.clones;
        let mut problems: Vec<SettingProblem> = clones
            .iter()
            .enumerate()
            .filter_map(|(row, clone)| {
                let problem = self.check_clone(clone, clones.get(..row)?)?;
                Some(SettingProblem::Clone { row, problem })
            })
            .collect();
        if let SummaryInstructionsChoice::OtherFile(path) = &self.edited.summary_instructions
            && !self.in_home(path).is_file()
        {
            problems.push(SettingProblem::SummaryFileGone(path.clone()));
        }
        problems
    }

    fn check_clone(&self, clone: &CloneRow, rows_before: &[CloneRow]) -> Option<CloneRowProblem> {
        let Some(repo) = clone.repo_name() else {
            return Some(CloneRowProblem::NotOwnerSlashRepo);
        };
        if rows_before.iter().any(|before| before.repo == clone.repo) {
            return Some(CloneRowProblem::SameRepoTwice);
        }
        LocalGit::configured_clone(&repo, &self.in_home(&clone.folder))
            .err()
            .map(CloneRowProblem::from)
    }

    /// `~/` in a setting is the home Settings was loaded from.
    fn in_home(&self, path: &Path) -> PathBuf {
        path.strip_prefix("~")
            .map_or_else(|_| path.to_path_buf(), |rest| self.home.join(rest))
    }

    pub fn has_unsaved_changes(&self) -> bool {
        self.edited != self.saved
    }

    pub fn save(&mut self) -> anyhow::Result<SaveOutcome> {
        if !self.has_unsaved_changes() {
            return Ok(SaveOutcome::NothingChanged);
        }
        let problems = self.check();
        if !problems.is_empty() {
            return Ok(SaveOutcome::Refused(problems));
        }
        let root = self.document.as_table_mut();
        let (saved, edited) = (&self.saved, &self.edited);
        if saved.ask_model != edited.ask_model {
            set_text(root, ASK_MODEL, edited.ask_model.as_deref());
        }
        if saved.ask_effort != edited.ask_effort {
            set_text(root, ASK_EFFORT, edited.ask_effort.as_deref());
        }
        if saved.ide != edited.ide {
            set_text(root, IDE, edited.ide.as_ref().map(IdeTemplate::as_str));
        }
        if saved.summary_instructions != edited.summary_instructions {
            set_value(
                root,
                SUMMARY_INSTRUCTIONS,
                edited.summary_instructions.to_value()?,
            );
        }
        if saved.clones != edited.clones {
            write_clones(root, &saved.clones, &edited.clones)?;
        }
        self.write()?;
        self.saved = self.edited.clone();
        Ok(SaveOutcome::ReopenOpenPr)
    }

    fn write(&self) -> anyhow::Result<()> {
        let folder = self
            .file
            .parent()
            .with_context(|| format!("{} has no folder", self.file.display()))?;
        std::fs::create_dir_all(folder)
            .with_context(|| format!("could not create {}", folder.display()))?;
        let temporary = self.file.with_extension("toml.tmp");
        std::fs::write(&temporary, self.document.to_string())
            .with_context(|| format!("could not write {}", temporary.display()))?;
        std::fs::rename(&temporary, &self.file)
            .with_context(|| format!("could not replace {}", self.file.display()))
    }
}

/// Touches only the entries that were added, removed or moved to another folder.
fn write_clones(root: &mut Table, saved: &[CloneRow], edited: &[CloneRow]) -> anyhow::Result<()> {
    let clones = root
        .entry(CLONES)
        .or_insert_with(|| Item::Table(Table::new()))
        .as_table_like_mut()
        .with_context(|| format!("{CLONES} is not a table"))?;
    for gone in saved
        .iter()
        .filter(|saved| !edited.iter().any(|edited| edited.repo == saved.repo))
    {
        clones.remove(&gone.repo);
    }
    for clone in edited.iter().filter(|edited| !saved.contains(edited)) {
        set_value(
            clones,
            &clone.repo,
            Some(Value::from(config_path(&clone.folder)?)),
        );
    }
    Ok(())
}

fn config_path(path: &Path) -> anyhow::Result<&str> {
    path.to_str()
        .with_context(|| format!("{} is not a path config.toml can hold", path.display()))
}

/// Keeps the spaces and comment around a value it replaces.
fn set_text(table: &mut dyn TableLike, key: &str, text: Option<&str>) {
    set_value(table, key, text.map(Value::from));
}

fn set_value(table: &mut dyn TableLike, key: &str, value: Option<Value>) {
    let Some(mut value) = value else {
        table.remove(key);
        return;
    };
    match table.get_mut(key) {
        Some(Item::Value(old)) => {
            *value.decor_mut() = old.decor().clone();
            *old = value;
        }
        Some(other) => *other = Item::Value(value),
        None => {
            table.insert(key, Item::Value(value));
        }
    }
}
