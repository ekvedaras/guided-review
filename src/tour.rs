use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::{Deserialize, Serialize};

use crate::pull_request::{PullRequestUrl, Sha};

const VERSION: u32 = 1;

/// The tour file: pointers only, pinned to a base and a head. Any viewer reads this.
#[derive(Debug, Serialize, Deserialize)]
pub struct Tour {
    #[serde(default = "current_version")]
    pub version: u32,
    #[serde(default)]
    pub pr: String,
    #[serde(default)]
    pub base: String,
    #[serde(default)]
    pub head: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_head: Option<String>,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub summary: Vec<String>,
    #[serde(default)]
    pub steps: Vec<Step>,
}

fn current_version() -> u32 {
    VERSION
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Step {
    pub id: String,
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
    pub children: Vec<Step>,
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

impl Tour {
    pub fn path_for(home: &Path, pr: &PullRequestUrl) -> PathBuf {
        home.join(".guided-review/tours")
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
        std::fs::write(path, text + "\n")
            .with_context(|| format!("could not write {}", path.display()))
    }

    /// The facts the tool owns; the tour writer never guesses them.
    pub fn pin(&mut self, pr: &PullRequestUrl, base: &Sha, head: &Sha) {
        self.version = VERSION;
        self.pr = pr.to_string();
        self.base = base.to_string();
        self.head = head.to_string();
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
}
