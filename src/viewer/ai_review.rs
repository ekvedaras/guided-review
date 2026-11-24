//! What the reviewer knows of the AI review of the subject they read: whether the review agent's
//! findings are in, and which. Read from the findings file beside the tour, never kept in the state file.

use crate::app_folders::AppFolders;
use crate::findings::{FindingCounts, FindingsFile};
use crate::review_subject::ReviewSubject;

#[derive(Debug)]
pub enum AiReview {
    NotRun,
    Findings(FindingsFile),
    /// The file is there but cannot be shown; the reason is for the reviewer and the agent.
    Unreadable(String),
}

impl AiReview {
    /// Findings a snapshot scene shows, as a review agent's file would hold them.
    #[cfg(feature = "snapshots")]
    pub fn staged() -> Self {
        let finding = |id: &str, severity: &str| serde_json::json!({"id": id, "severity": severity, "body": "Staged finding.", "anchor": {"kind": "change"}});
        let file = serde_json::json!({"findings": [
            finding("a", "must fix"),
            finding("b", "should fix"),
            finding("c", "should fix"),
            finding("d", "nit"),
        ]});
        serde_json::from_value(file).map_or(Self::NotRun, Self::Findings)
    }

    /// Findings of every severity on the line a snapshot scene shows, as a review agent's file would hold them.
    #[cfg(feature = "snapshots")]
    pub fn staged_on(
        path: &crate::file_path::FilePath,
        line: crate::line_number::LineNumber,
    ) -> Self {
        let finding = |id: &str, severity: &str, body: &str| {
            serde_json::json!({"id": id, "severity": severity, "body": body, "anchor": {
                "kind": "lines", "file": path, "side": "head", "lines": [line, line],
            }})
        };
        let file = serde_json::json!({"findings": [
            finding("a", "must fix", "This can fail when the list is empty.\n\nGuard it before taking the first item."),
            finding("b", "nit", "Prefer the shorter name here."),
        ]});
        serde_json::from_value(file).map_or(Self::NotRun, Self::Findings)
    }

    /// A finding a new commit made outdated, on the line a snapshot scene shows.
    #[cfg(feature = "snapshots")]
    pub fn staged_outdated_on(
        path: &crate::file_path::FilePath,
        line: crate::line_number::LineNumber,
    ) -> Self {
        let file = serde_json::json!({"findings": [{
            "id": "a", "severity": "must fix",
            "body": "This can fail when the list is empty.\n\nGuard it before taking the first item.",
            "anchor": {"kind": "lines", "file": path, "side": "head", "lines": [line, line]},
            "outdated": {"before": ["let first = items[0];"]},
        }]});
        serde_json::from_value(file).map_or(Self::NotRun, Self::Findings)
    }

    /// A file that cannot be read is shown as such, never an error: the review opens without it.
    pub fn read(folders: &AppFolders, subject: &ReviewSubject) -> Self {
        match FindingsFile::read_of(folders, subject) {
            Ok(Some(file)) => Self::Findings(file),
            Ok(None) => Self::NotRun,
            Err(error) => Self::Unreadable(format!("{error:#}")),
        }
    }

    pub fn counts(&self) -> FindingCounts {
        match self {
            Self::Findings(file) => file.counts(),
            Self::NotRun | Self::Unreadable(_) => FindingCounts::default(),
        }
    }

    /// The AI review card's line: the findings by severity, as the card's head counts them.
    pub fn line(&self) -> String {
        match self {
            Self::NotRun => "Not run".to_owned(),
            Self::Unreadable(reason) => format!("Not read: {reason}"),
            Self::Findings(_) => {
                let counts = self.counts();
                let by_severity = [
                    (counts.must_fix, "must fix"),
                    (counts.should_fix, "should fix"),
                    (counts.nit, "nit"),
                ]
                .into_iter()
                .filter(|(count, _)| *count > 0)
                .map(|(count, name)| format!("{count} {name}"))
                .collect::<Vec<_>>()
                .join(", ");
                match counts.total() {
                    0 => "No findings".to_owned(),
                    _ => by_severity,
                }
            }
        }
    }
}
