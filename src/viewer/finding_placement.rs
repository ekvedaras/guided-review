//! Which screen shows which finding of the AI review: the step (or file) that shows its lines, the safety
//! net for lines no step shows, the first screen for a finding about the whole change.

use std::collections::BTreeMap;

use crate::findings::{Anchor, Finding, FindingId};
use crate::pieces::ChangedFile;
use crate::tour::Tour;

use super::diff_display::DiffDisplay;
use super::finding_threads::{FindingThread, FindingThreads};
use super::fix_requests::{FixRequests, FixState};
use super::review::StepId;
use super::step_page::{LineLocation, Side};
use super::threads::{hangs_under_a_row, step_pieces};
use crate::file_path::FilePath;

/// A finding as one screen shows it.
#[derive(Debug, Clone)]
pub struct FindingView {
    pub finding: Finding,
    /// The shown line it hangs under; `None` for a finding shown after the code.
    pub anchor: Option<LineLocation>,
    /// The fix the reviewer asked for, when one was.
    pub fix: Option<FixState>,
    /// Ask why on it, once the reviewer asked.
    pub thread: Option<FindingThread>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum FindingHome {
    Step(StepId),
    SafetyNet,
    File(FilePath),
    /// The tour's first screen: the summary, or a basic tour's overview.
    Change,
}

#[derive(Debug, Default)]
pub(super) struct FindingPlacement {
    homes: BTreeMap<FindingHome, Vec<FindingView>>,
}

impl FindingPlacement {
    pub fn of(
        tour: &Tour,
        files: &[ChangedFile],
        findings: &[Finding],
        display: &DiffDisplay,
        fixes: &FixRequests,
        threads: &FindingThreads,
    ) -> Self {
        let steps = tour
            .guided()
            .map(|guided| step_pieces(guided, files, display));
        let mut homes: BTreeMap<FindingHome, Vec<FindingView>> = BTreeMap::new();
        for finding in findings {
            let anchor = hangs_at(&finding.anchor);
            let home = match (&finding.anchor, &steps) {
                (Anchor::Change, _) => FindingHome::Change,
                (Anchor::File { file } | Anchor::Lines { file, .. }, _)
                    if !files.iter().any(|changed| changed.path == *file) =>
                {
                    FindingHome::Change
                }
                (Anchor::File { file } | Anchor::Lines { file, .. }, None) => {
                    FindingHome::File(file.clone())
                }
                (Anchor::File { file }, Some(steps)) => steps
                    .iter()
                    .find(|(_, parts)| parts.iter().any(|piece| piece.id.path() == file))
                    .map_or(FindingHome::SafetyNet, |(id, _)| {
                        FindingHome::Step(id.clone())
                    }),
                (Anchor::Lines { .. }, Some(steps)) => steps
                    .iter()
                    .find(|(_, parts)| {
                        anchor.as_ref().is_some_and(|location| {
                            parts.iter().any(|piece| hangs_under_a_row(piece, location))
                        })
                    })
                    .map_or(FindingHome::SafetyNet, |(id, _)| {
                        FindingHome::Step(id.clone())
                    }),
            };
            homes.entry(home).or_default().push(FindingView {
                finding: finding.clone(),
                anchor,
                fix: fixes.state_of(&finding.id).cloned(),
                thread: threads.thread_of(&finding.id).cloned(),
            });
        }
        Self { homes }
    }

    pub fn home_of(&self, id: &FindingId) -> Option<&FindingHome> {
        self.homes
            .iter()
            .find(|(_, views)| views.iter().any(|view| view.finding.id == *id))
            .map(|(home, _)| home)
    }

    pub fn in_home(&self, home: &FindingHome) -> &[FindingView] {
        self.homes.get(home).map_or(&[], Vec::as_slice)
    }
}

/// The line a finding about lines hangs under: the last of them, on their side.
fn hangs_at(anchor: &Anchor) -> Option<LineLocation> {
    let Anchor::Lines { file, side, lines } = anchor else {
        return None;
    };
    Some(LineLocation {
        path: file.clone(),
        side: Side::from(*side),
        line: lines.end,
    })
}
