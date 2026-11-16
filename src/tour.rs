use std::fmt;
use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::{Deserialize, Serialize};

use crate::app_folders::AppFolders;
use crate::file_path::FilePath;
use crate::piece_id::PieceId;
use crate::pull_request::{PullRequestTips, PullRequestUrl, Sha};

const VERSION: u32 = 1;

/// A tour is one of two kinds, told apart by type: the basic tour the app makes with no AI, and
/// the guided tour the tour writer wrote.
#[derive(Debug)]
pub enum Tour {
    Basic(BasicTour),
    Guided(GuidedTour),
}

/// Which tour files a PR has on disk. A PR with both shows its basic tour, the guided one being ready.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToursOnDisk {
    None,
    Basic,
    Guided,
    BasicWithGuided,
}

impl ToursOnDisk {
    /// A PR with no tour opens as a basic one, and with both files the basic one is shown.
    pub fn opens_basic(self) -> bool {
        self != Self::Guided
    }

    pub fn of(folders: &AppFolders, pr: &PullRequestUrl) -> Self {
        match (
            Tour::basic_path_for(folders, pr).exists(),
            Tour::path_for(folders, pr).exists(),
        ) {
            (false, false) => Self::None,
            (true, false) => Self::Basic,
            (false, true) => Self::Guided,
            (true, true) => Self::BasicWithGuided,
        }
    }
}

/// What both kinds are pinned to. Any viewer reads this.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TourPin {
    #[serde(default = "current_version")]
    pub version: u32,
    #[serde(default)]
    pub pr: String,
    #[serde(default)]
    pub base: Sha,
    #[serde(default)]
    pub head: Sha,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_head: Option<Sha>,
}

/// The tour file of a guided tour: pointers only, pinned to a base and a head.
#[derive(Debug, Serialize, Deserialize)]
pub struct GuidedTour {
    #[serde(flatten)]
    pub pin: TourPin,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub summary: Vec<String>,
    /// Markdown the tour writer wrote to the reviewer's summary instructions.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub reviewer_summary: String,
    #[serde(default)]
    pub steps: Vec<Step>,
}

/// The tour file of a basic tour: the pin and the PR's own title, with no steps and no summary.
#[derive(Debug, Clone, Serialize)]
pub struct BasicTour {
    #[serde(flatten)]
    pub pin: TourPin,
    pub pr_title: String,
    kind: BasicKind,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum BasicKind {
    Basic,
}

#[derive(Deserialize)]
struct BasicFile {
    #[serde(flatten)]
    pin: TourPin,
    #[serde(default)]
    pr_title: String,
    #[serde(rename = "kind")]
    _kind: BasicKind,
}

/// What a basic file must not hold: only a guided tour has them.
const GUIDED_ONLY_FIELDS: [&str; 4] = ["title", "summary", "reviewer_summary", "steps"];

/// What a tour file says it is. A file with no `kind` is a guided tour, as every file written
/// before the basic tour existed.
#[derive(Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum KindTag {
    Basic,
    #[default]
    Guided,
}

#[derive(Deserialize)]
struct KindProbe {
    #[serde(default)]
    kind: KindTag,
}

impl BasicTour {
    pub const fn new(pin: TourPin, pr_title: String) -> Self {
        Self {
            pin,
            pr_title,
            kind: BasicKind::Basic,
        }
    }
}

impl TryFrom<serde_json::Value> for BasicTour {
    type Error = anyhow::Error;

    fn try_from(value: serde_json::Value) -> anyhow::Result<Self> {
        if let Some(field) = GUIDED_ONLY_FIELDS
            .iter()
            .find(|field| value.get(**field).is_some())
        {
            anyhow::bail!("a basic tour has no `{field}`; that belongs to a guided tour");
        }
        let file: BasicFile = serde_json::from_value(value)?;
        Ok(Self::new(file.pin, file.pr_title))
    }
}

fn current_version() -> u32 {
    VERSION
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct StepId(String);

impl From<&str> for StepId {
    fn from(id: &str) -> Self {
        Self(id.to_owned())
    }
}

impl fmt::Display for StepId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Step {
    pub id: StepId,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub why: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub check: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tldr: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diagram: Option<String>,
    #[serde(default)]
    pub skim: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pieces: Vec<PieceRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<Self>,
}

/// What a step points at. Variants are tried in this order when reading.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PieceRef {
    /// Every change under a folder that no other pointer covers.
    Rest {
        rest: String,
    },
    /// A line range, for files the tool cannot cut into symbols.
    Lines {
        file: String,
        lines: [u32; 2],
    },
    Symbol {
        file: String,
        symbol: String,
    },
    WholeFile {
        file: String,
    },
}

impl PieceRef {
    /// The one piece a pointer names; a range or a folder names none, and a pointer that is no
    /// valid piece id names none either, so the tour check reports it as not found.
    pub fn piece_id(&self) -> Option<PieceId> {
        match self {
            Self::Symbol { file, symbol } => {
                PieceId::symbol(FilePath::new(file).ok()?, symbol.as_str()).ok()
            }
            Self::WholeFile { file } => Some(PieceId::whole_file(FilePath::new(file).ok()?)),
            Self::Rest { .. } | Self::Lines { .. } => None,
        }
    }
}

impl Tour {
    pub const TOURS_DIR_ENV: &'static str = "GUIDED_REVIEW_TOURS_DIR";

    /// `tours` in the state folder, unless `GUIDED_REVIEW_TOURS_DIR` points the commands elsewhere.
    pub fn tours_dir(folders: &AppFolders) -> PathBuf {
        std::env::var_os(Self::TOURS_DIR_ENV).map_or_else(|| folders.tours(), PathBuf::from)
    }

    /// Where the guided tour lives. This is the path `tour-path` prints: the tour writer, `check`
    /// and `refresh` only ever see this file.
    pub fn path_for(folders: &AppFolders, pr: &PullRequestUrl) -> PathBuf {
        Self::path_in(&Self::tours_dir(folders), pr)
    }

    /// Where the basic tour lives, beside the guided tour's file.
    pub fn basic_path_for(folders: &AppFolders, pr: &PullRequestUrl) -> PathBuf {
        Self::basic_path_in(&Self::tours_dir(folders), pr)
    }

    /// Where the tour writer keeps files the tour shows, like the pictures of its summary.
    pub fn files_for(folders: &AppFolders, pr: &PullRequestUrl) -> PathBuf {
        Self::tours_dir(folders)
            .join(pr.repo.to_string())
            .join(pr.number.to_string())
    }

    pub fn path_in(tours_dir: &Path, pr: &PullRequestUrl) -> PathBuf {
        tours_dir
            .join(pr.repo.to_string())
            .join(format!("{}.json", pr.number))
    }

    pub fn basic_path_in(tours_dir: &Path, pr: &PullRequestUrl) -> PathBuf {
        tours_dir
            .join(pr.repo.to_string())
            .join(format!("{}.basic.json", pr.number))
    }

    pub const fn pin(&self) -> &TourPin {
        match self {
            Self::Basic(tour) => &tour.pin,
            Self::Guided(tour) => &tour.pin,
        }
    }

    /// `None` for a basic tour, which has no steps, summary or reviewer summary.
    pub const fn guided(&self) -> Option<&GuidedTour> {
        match self {
            Self::Basic(_) => None,
            Self::Guided(tour) => Some(tour),
        }
    }

    /// Reads a tour file of either kind; a file with no `kind` is a guided tour.
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let value = read_json(path)?;
        let invalid = || format!("{} is not a valid tour file", path.display());
        let probe: KindProbe = serde_json::from_value(value.clone()).with_context(invalid)?;
        match probe.kind {
            KindTag::Basic => BasicTour::try_from(value).map(Self::Basic),
            KindTag::Guided => serde_json::from_value(value)
                .map(Self::Guided)
                .map_err(anyhow::Error::from),
        }
        .with_context(invalid)
    }

    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        match self {
            Self::Basic(tour) => tour.save(path),
            Self::Guided(tour) => tour.save(path),
        }
    }
}

fn read_json(path: &Path) -> anyhow::Result<serde_json::Value> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("could not read the tour file {}", path.display()))?;
    serde_json::from_str(&text)
        .with_context(|| format!("{} is not a valid tour file", path.display()))
}

fn write_json(value: &impl Serialize, path: &Path) -> anyhow::Result<()> {
    let text = serde_json::to_string_pretty(value)?;
    let mut temp_name = path.as_os_str().to_owned();
    temp_name.push(".tmp");
    let temp = PathBuf::from(temp_name);
    std::fs::write(&temp, text + "\n")
        .and_then(|()| std::fs::rename(&temp, path))
        .with_context(|| format!("could not write {}", path.display()))
}

impl TourPin {
    /// The facts the tool owns; the tour writer never guesses them.
    pub fn pin_to(&mut self, pr: &PullRequestUrl, base: &Sha, head: &Sha) {
        self.version = VERSION;
        self.pr = pr.to_string();
        self.base = base.clone();
        self.head = head.clone();
    }

    pub fn is_pinned(&self) -> bool {
        !self.base.is_empty() && !self.head.is_empty()
    }

    pub fn pinned_tips(&self) -> PullRequestTips {
        PullRequestTips {
            base_branch_tip: self.base.clone(),
            head: self.head.clone(),
        }
    }
}

impl BasicTour {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        Self::try_from(read_json(path)?)
            .with_context(|| format!("{} is not a valid basic tour file", path.display()))
    }

    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        write_json(self, path)
    }
}

impl GuidedTour {
    /// Reads the guided tour's file: no `kind`, or `"kind": "guided"`.
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        match Tour::load(path)? {
            Tour::Guided(tour) => Ok(tour),
            Tour::Basic(_) => {
                anyhow::bail!("{} holds a basic tour, not a guided one", path.display())
            }
        }
    }

    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        write_json(self, path)
    }

    /// Every step, parents before their children.
    pub fn steps(&self) -> Vec<&Step> {
        fn collect<'a>(steps: &'a [Step], into: &mut Vec<&'a Step>) {
            for step in steps {
                into.push(step);
                collect(&step.children, into);
            }
        }
        let mut all = Vec::new();
        collect(&self.steps, &mut all);
        all
    }

    pub fn find_step(&self, id: &StepId) -> Option<&Step> {
        fn find<'a>(steps: &'a [Step], id: &StepId) -> Option<&'a Step> {
            steps.iter().find_map(|step| {
                (step.id == *id)
                    .then_some(step)
                    .or_else(|| find(&step.children, id))
            })
        }
        find(&self.steps, id)
    }
}
