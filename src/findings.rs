//! The findings file: what a review agent reports for one review subject, kept beside its tour.
//! The agent writes it whole, `check-findings` checks it, and `findings-done` tells the app.

use std::fmt;
use std::path::Path;

use anyhow::Context;
use serde::{Deserialize, Serialize};

use crate::app_folders::AppFolders;
use crate::file_path::FilePath;
use crate::line_number::LineNumber;
use crate::review_subject::ReviewSubject;
use crate::tour::{Tour, TourPin, write_json};

/// How much a finding matters, which orders the reviewer's walk through them: most severe first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Severity {
    #[serde(rename = "must fix")]
    MustFix,
    #[serde(rename = "should fix")]
    ShouldFix,
    #[serde(rename = "nit")]
    Nit,
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::MustFix => "must fix",
            Self::ShouldFix => "should fix",
            Self::Nit => "nit",
        })
    }
}

/// Names a finding inside its file, so a later run can be told which ones the reviewer dismissed.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct FindingId(String);

impl TryFrom<String> for FindingId {
    type Error = String;

    fn try_from(id: String) -> Result<Self, String> {
        if id.trim().is_empty() {
            return Err("a finding needs an id".to_owned());
        }
        Ok(Self(id))
    }
}

impl From<FindingId> for String {
    fn from(id: FindingId) -> Self {
        id.0
    }
}

impl fmt::Display for FindingId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// The side of a file's diff a finding's lines are numbered on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AnchorSide {
    Base,
    Head,
}

impl fmt::Display for AnchorSide {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Base => "base",
            Self::Head => "head",
        })
    }
}

/// What a finding is about: lines of one side, a whole file, or the whole change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Anchor {
    Change,
    File {
        file: FilePath,
    },
    Lines {
        file: FilePath,
        side: AnchorSide,
        lines: [LineNumber; 2],
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Finding {
    pub id: FindingId,
    pub severity: Severity,
    /// Markdown; its first line is what a list shows of the finding.
    pub body: String,
    pub anchor: Anchor,
}

/// The findings file, pinned to the base and head the review agent read, like a tour is.
#[derive(Debug, Serialize, Deserialize)]
pub struct FindingsFile {
    #[serde(flatten)]
    pub pin: TourPin,
    #[serde(default)]
    pub findings: Vec<Finding>,
}

impl FindingsFile {
    /// Reads the subject's findings file. `None` when the agent has written none.
    pub fn read_of(folders: &AppFolders, subject: &ReviewSubject) -> anyhow::Result<Option<Self>> {
        let path = Tour::findings_path_for(folders, subject);
        if !path.exists() {
            return Ok(None);
        }
        Self::load_of(&path, subject).map(Some)
    }

    pub fn load_of(path: &Path, subject: &ReviewSubject) -> anyhow::Result<Self> {
        let text = std::fs::read_to_string(path).with_context(|| {
            format!(
                "no findings file at {}: write it there, then run check-findings again",
                path.display()
            )
        })?;
        let file: Self = serde_json::from_str(&text)
            .with_context(|| format!("{} is not a valid findings file", path.display()))?;
        file.pin.ensure_is_of(subject, path)?;
        Ok(file)
    }

    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        write_json(self, path)
    }

    /// How many findings of each severity, most severe first, for the sidebar's line.
    pub fn counts(&self) -> FindingCounts {
        let of = |severity| {
            self.findings
                .iter()
                .filter(|f| f.severity == severity)
                .count()
        };
        FindingCounts {
            must_fix: of(Severity::MustFix),
            should_fix: of(Severity::ShouldFix),
            nit: of(Severity::Nit),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FindingCounts {
    pub must_fix: usize,
    pub should_fix: usize,
    pub nit: usize,
}

impl FindingCounts {
    pub const fn total(self) -> usize {
        self.must_fix + self.should_fix + self.nit
    }
}
