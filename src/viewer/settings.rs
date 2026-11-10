use std::path::{Path, PathBuf};

use anyhow::Context;
use toml_edit::{DocumentMut, InlineTable, Item, Table, TableLike, Value};

use crate::config::{AskEffortChoice, AskModelChoice, Config};
use crate::home::expand_home;
use crate::ide::{IdeChoice, IdeTemplate};
use crate::local_git::{CloneProblem, LocalGit};
use crate::pull_request::RepoName;
use crate::summary_instructions::{SkillName, SummaryInstructionsChoice};

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
    ask_model: AskModelChoice,
    ask_effort: AskEffortChoice,
    ide: Option<IdeTemplate>,
    summary_instructions: SummaryInstructionsChoice,
    clones: Vec<CloneRow>,
}

impl Values {
    fn read(document: &DocumentMut) -> Self {
        let text = |key: &str| document.get(key).and_then(Item::as_str).map(str::to_owned);
        Self {
            ask_model: AskModelChoice::of(text(ASK_MODEL).as_deref()),
            ask_effort: AskEffortChoice::of(text(ASK_EFFORT).as_deref()),
            ide: text(IDE).map(IdeTemplate::new),
            summary_instructions: summary_instructions_in(document),
            clones: document
                .get(CLONES)
                .and_then(Item::as_table_like)
                .into_iter()
                .flat_map(TableLike::iter)
                .filter_map(|(repo, folder)| {
                    Some(CloneRow::new(repo, PathBuf::from(folder.as_str()?)))
                })
                .collect(),
        }
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

    pub fn folder_label(&self) -> String {
        if self.folder.as_os_str().is_empty() {
            "No folder yet".to_owned()
        } else {
            self.folder.display().to_string()
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
        clone: CloneRow,
        problem: CloneRowProblem,
    },
    SummaryFileGone(PathBuf),
    /// The config holds a `summary_instructions` that is not a path, `"off"` or a skill name; the value as written.
    SummaryInstructionsUnreadable(String),
    /// The config holds an `ask_effort` that `claude --effort` does not accept.
    AskEffortUnknown(String),
    /// Custom is picked and no model is typed.
    AskModelBlank,
    /// Custom is picked and no template is typed.
    IdeTemplateBlank,
}

impl SettingProblem {
    /// The red line under the setting.
    pub fn message(&self) -> String {
        match self {
            Self::Clone { clone, problem, .. } => problem.message(clone),
            Self::SummaryFileGone(path) => format!("The summary file {} is gone", path.display()),
            Self::SummaryInstructionsUnreadable(written) => {
                format!(
                    "{written} is not a file, \"{}\" or a skill name",
                    SummaryInstructionsChoice::OFF
                )
            }
            Self::AskModelBlank => "Type the model name, or pick another choice".to_owned(),
            Self::IdeTemplateBlank => {
                "Type the command or link, with {path} and {line}, or pick another choice"
                    .to_owned()
            }
            Self::AskEffortUnknown(text) => {
                format!("{text:?} is not an effort Claude accepts: low, medium, high, xhigh or max")
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloneRowProblem {
    NotOwnerSlashRepo,
    SameRepoTwice,
    FolderGone,
    NoRemoteOfRepo,
}

impl CloneRowProblem {
    pub fn message(self, clone: &CloneRow) -> String {
        match self {
            Self::NotOwnerSlashRepo => "Write the repo as owner/repo".to_owned(),
            Self::SameRepoTwice => format!("{} is set twice", clone.repo),
            Self::FolderGone => "This folder is gone".to_owned(),
            Self::NoRemoteOfRepo => {
                format!("No remote of this clone points at {}", clone.repo)
            }
        }
    }
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
const SKILLS_FOLDER: &str = ".claude/skills";

/// A document `toml_edit` accepts and `toml` refuses holds something this config cannot read, so the setting says so.
fn summary_instructions_in(document: &DocumentMut) -> SummaryInstructionsChoice {
    let Some(item) = document.get(SUMMARY_INSTRUCTIONS) else {
        return SummaryInstructionsChoice::DefaultFile;
    };
    document
        .to_string()
        .parse::<toml::Table>()
        .ok()
        .and_then(|table| {
            table
                .get(SUMMARY_INSTRUCTIONS)
                .map(SummaryInstructionsChoice::from_value)
        })
        .unwrap_or_else(|| {
            SummaryInstructionsChoice::Unreadable(item.to_string().trim().to_owned())
        })
}

/// Writing is all Settings adds to what the domain reads.
fn summary_instructions_value(choice: &SummaryInstructionsChoice) -> anyhow::Result<Option<Value>> {
    Ok(match choice {
        SummaryInstructionsChoice::DefaultFile => None,
        SummaryInstructionsChoice::Off => Some(Value::from(SummaryInstructionsChoice::OFF)),
        SummaryInstructionsChoice::OtherFile(path) => Some(Value::from(config_path(path)?)),
        SummaryInstructionsChoice::Skill(name) => {
            let mut table = InlineTable::new();
            table.insert(
                SummaryInstructionsChoice::SKILL_KEY,
                Value::from(name.as_str()),
            );
            Some(Value::InlineTable(table))
        }
        SummaryInstructionsChoice::Unreadable(written) => {
            anyhow::bail!("{written} is not a {SUMMARY_INSTRUCTIONS} that can be saved")
        }
    })
}

#[derive(Debug, PartialEq, Eq)]
pub enum SaveOutcome {
    NothingChanged,
    /// The config file was written; the open PR is opened again so the new settings apply.
    Written,
    /// Nothing was written; each problem goes under its setting.
    Refused(Vec<SettingProblem>),
}

impl Settings {
    pub fn load(home: &Path) -> anyhow::Result<Self> {
        let file = Config::file_in(home);
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
        let saved = Values::read(&document);
        Ok(Self {
            home: home.to_path_buf(),
            file,
            document,
            edited: saved.clone(),
            saved,
        })
    }

    pub fn ask_model(&self) -> AskModelChoice {
        self.edited.ask_model.clone()
    }

    pub fn set_ask_model(&mut self, model: &AskModelChoice) {
        self.edited.ask_model = model.clone();
    }

    pub fn ask_effort(&self) -> AskEffortChoice {
        self.edited.ask_effort.clone()
    }

    pub fn set_ask_effort(&mut self, effort: &AskEffortChoice) {
        self.edited.ask_effort = effort.clone();
    }

    pub fn ide(&self) -> IdeChoice {
        IdeChoice::of(self.edited.ide.as_ref())
    }

    pub fn set_ide(&mut self, ide: &IdeChoice) {
        self.edited.ide = ide.template();
    }

    pub fn summary_instructions(&self) -> &SummaryInstructionsChoice {
        &self.edited.summary_instructions
    }

    pub fn set_summary_instructions(&mut self, choice: SummaryInstructionsChoice) {
        self.edited.summary_instructions = choice;
    }

    pub fn default_file(&self) -> PathBuf {
        SummaryInstructionsChoice::default_file_in(&self.home)
    }

    pub fn default_file_exists(&self) -> bool {
        self.default_file().is_file()
    }

    /// Makes an empty default file when there is none, and returns its path to open.
    pub fn create_default_file(&self) -> anyhow::Result<PathBuf> {
        let file = self.default_file();
        let folder = file
            .parent()
            .with_context(|| format!("{} has no folder", file.display()))?;
        std::fs::create_dir_all(folder)
            .with_context(|| format!("could not create {}", folder.display()))?;
        std::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(&file)
            .with_context(|| format!("could not create {}", file.display()))?;
        Ok(file)
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
            .filter_map(|entry| SkillName::parse(entry.file_name().to_str()?).ok())
            .collect();
        skills.sort();
        skills
    }

    /// The problems the config already has in it. The rest show once Save refuses them.
    pub fn problems_on_open(&self) -> Vec<SettingProblem> {
        self.check()
            .into_iter()
            .filter(|problem| {
                matches!(
                    problem,
                    SettingProblem::SummaryInstructionsUnreadable(_)
                        | SettingProblem::AskEffortUnknown(_)
                )
            })
            .collect()
    }

    /// One problem per setting that would be wrong if saved.
    pub fn check(&self) -> Vec<SettingProblem> {
        let clones = &self.edited.clones;
        let mut problems: Vec<SettingProblem> = clones
            .iter()
            .enumerate()
            .filter_map(|(row, clone)| {
                let problem = self.check_clone(clone, clones.get(..row)?)?;
                Some(SettingProblem::Clone {
                    row,
                    clone: clone.clone(),
                    problem,
                })
            })
            .collect();
        if let AskEffortChoice::Unknown(text) = self.ask_effort() {
            problems.push(SettingProblem::AskEffortUnknown(text));
        }
        if matches!(self.ask_model(), AskModelChoice::Custom(text) if text.trim().is_empty()) {
            problems.push(SettingProblem::AskModelBlank);
        }
        if matches!(self.ide(), IdeChoice::Custom(template) if template.as_str().trim().is_empty())
        {
            problems.push(SettingProblem::IdeTemplateBlank);
        }
        match &self.edited.summary_instructions {
            SummaryInstructionsChoice::OtherFile(path) if !self.in_home(path).is_file() => {
                problems.push(SettingProblem::SummaryFileGone(path.clone()));
            }
            SummaryInstructionsChoice::Unreadable(written) => problems.push(
                SettingProblem::SummaryInstructionsUnreadable(written.clone()),
            ),
            _ => {}
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

    fn in_home(&self, path: &Path) -> PathBuf {
        expand_home(path, &self.home)
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
            set_text(root, ASK_MODEL, edited.ask_model.config_text());
        }
        if saved.ask_effort != edited.ask_effort {
            set_text(root, ASK_EFFORT, edited.ask_effort.config_text());
        }
        if saved.ide != edited.ide {
            set_text(root, IDE, edited.ide.as_ref().map(IdeTemplate::as_str));
        }
        if saved.summary_instructions != edited.summary_instructions {
            set_value(
                root,
                SUMMARY_INSTRUCTIONS,
                summary_instructions_value(&edited.summary_instructions)?,
            );
        }
        if saved.clones != edited.clones {
            write_clones(root, &saved.clones, &edited.clones)?;
        }
        self.write()?;
        self.saved = self.edited.clone();
        Ok(SaveOutcome::Written)
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
