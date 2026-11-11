use std::fmt;
use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::{Deserialize, Serialize};

use crate::file_path::FilePath;
use crate::piece_id::PieceId;
use crate::pull_request::{PullRequestTips, PullRequestUrl, Sha};

const VERSION: u32 = 1;

/// The tour file: pointers only, pinned to a base and a head. Any viewer reads this.
#[derive(Debug, Serialize, Deserialize)]
pub struct Tour {
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

    /// `~/.guided-review/tours`, unless `GUIDED_REVIEW_TOURS_DIR` points the commands elsewhere.
    pub fn tours_dir(home: &Path) -> PathBuf {
        std::env::var_os(Self::TOURS_DIR_ENV)
            .map_or_else(|| home.join(".guided-review/tours"), PathBuf::from)
    }

    pub fn path_for(home: &Path, pr: &PullRequestUrl) -> PathBuf {
        Self::path_in(&Self::tours_dir(home), pr)
    }

    /// Where the tour writer keeps files the tour shows, like the pictures of its summary.
    pub fn files_for(home: &Path, pr: &PullRequestUrl) -> PathBuf {
        Self::tours_dir(home)
            .join(pr.repo.to_string())
            .join(pr.number.to_string())
    }

    pub fn path_in(tours_dir: &Path, pr: &PullRequestUrl) -> PathBuf {
        tours_dir
            .join(pr.repo.to_string())
            .join(format!("{}.json", pr.number))
    }

    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("could not read the tour file {}", path.display()))?;
        serde_json::from_str(&text)
            .with_context(|| format!("{} is not a valid tour file", path.display()))
    }

    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        let text = serde_json::to_string_pretty(self)?;
        let mut temp_name = path.as_os_str().to_owned();
        temp_name.push(".tmp");
        let temp = PathBuf::from(temp_name);
        std::fs::write(&temp, text + "\n")
            .and_then(|()| std::fs::rename(&temp, path))
            .with_context(|| format!("could not write {}", path.display()))
    }

    /// The facts the tool owns; the tour writer never guesses them.
    pub fn pin(&mut self, pr: &PullRequestUrl, base: &Sha, head: &Sha) {
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
