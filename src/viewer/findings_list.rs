//! The findings list: one compact row per open finding, for going through many of them fast. It
//! holds what the reviewer has ticked and which row the keys are on, and nothing the findings file holds.

use std::collections::{BTreeMap, BTreeSet};

use super::fix_requests::FixRequests;
use crate::findings::{Anchor, Finding, FindingId, Severity};

/// One line of the code a finding is about, as the step shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FindingLine {
    pub number: u32,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FindingsListRow {
    pub id: FindingId,
    pub severity: Severity,
    /// `file:line`, the file, or the whole change.
    pub place: String,
    pub first_line: String,
    /// The whole finding, which an open row shows.
    pub body: String,
    pub is_selected: bool,
    pub is_open: bool,
    pub is_under_cursor: bool,
    /// The code lines of its anchor, which an open row shows; none for a finding on a file or the change.
    pub lines: Vec<FindingLine>,
    /// Where the fix the reviewer asked for is, when one was asked for.
    pub fix: Option<String>,
    /// New commits changed its lines since the review agent wrote it.
    pub is_outdated: bool,
}

#[derive(Debug, Default, Clone)]
pub struct FindingsList {
    findings: Vec<Finding>,
    selected: BTreeSet<FindingId>,
    cursor: usize,
    open: Option<FindingId>,
    /// What each finding's fix says, for the findings that have one.
    fixes: BTreeMap<FindingId, String>,
    /// What the reviewer dismissed, which can be restored.
    dismissed: Vec<Finding>,
    dismissed_selected: BTreeSet<FindingId>,
    dismissed_are_shown: bool,
    /// The code lines each finding is about.
    lines: BTreeMap<FindingId, Vec<FindingLine>>,
}

impl FindingsList {
    /// Most severe first, the findings file's order within a severity.
    pub fn of(findings: &[Finding]) -> Self {
        let mut findings = findings.to_vec();
        findings.sort_by_key(|finding| finding.severity);
        Self {
            findings,
            ..Self::default()
        }
    }

    /// The list with where each finding's fix is.
    #[must_use]
    pub fn with_fixes(mut self, fixes: &FixRequests) -> Self {
        self.fixes = self
            .findings
            .iter()
            .filter_map(|finding| {
                fixes
                    .state_of(&finding.id)
                    .map(|state| (finding.id.clone(), state.line()))
            })
            .collect();
        self
    }

    /// The list with the findings the reviewer dismissed, to look at and restore. They show folded, unless
    /// nothing is open.
    #[must_use]
    pub fn with_dismissed(mut self, dismissed: Vec<Finding>) -> Self {
        self.dismissed_are_shown = self.findings.is_empty() && !dismissed.is_empty();
        self.dismissed = dismissed;
        self
    }

    pub fn dismissed_rows(&self) -> Vec<FindingsListRow> {
        self.dismissed
            .iter()
            .enumerate()
            .map(|(position, finding)| FindingsListRow {
                is_selected: self.dismissed_selected.contains(&finding.id),
                is_under_cursor: self.dismissed_are_shown
                    && position + self.findings.len() == self.cursor,
                ..Self::row_of(finding)
            })
            .collect()
    }

    pub const fn dismissed_are_shown(&self) -> bool {
        self.dismissed_are_shown
    }

    pub fn toggle_dismissed_shown(&mut self) {
        self.dismissed_are_shown = !self.dismissed_are_shown;
        self.cursor = self.cursor.min(self.last_row());
    }

    /// The dismissed row's own tick: an id may be open and dismissed at once, so it is not told by id alone.
    pub fn toggle_dismissed_selected(&mut self, id: &FindingId) {
        if !self.dismissed_selected.remove(id) {
            self.dismissed_selected.insert(id.clone());
        }
    }

    /// In the dismissed section's order.
    pub fn selected_dismissed(&self) -> Vec<FindingId> {
        self.dismissed
            .iter()
            .filter(|finding| self.dismissed_selected.contains(&finding.id))
            .map(|finding| finding.id.clone())
            .collect()
    }

    /// A click on a dismissed row puts the cursor there, as long as the section is shown.
    pub fn move_cursor_to_dismissed(&mut self, id: &FindingId) {
        if !self.dismissed_are_shown {
            return;
        }
        if let Some(position) = self.dismissed.iter().position(|finding| finding.id == *id) {
            self.cursor = self.findings.len() + position;
        }
    }

    /// The cursor's row, when it is one of the open findings.
    const fn cursor_is_on_open(&self) -> bool {
        self.cursor < self.findings.len()
    }

    /// The last row the cursor can be on: the open findings, and the dismissed ones while shown.
    fn last_row(&self) -> usize {
        let shown_dismissed = if self.dismissed_are_shown {
            self.dismissed.len()
        } else {
            0
        };
        (self.findings.len() + shown_dismissed).saturating_sub(1)
    }

    /// The list with the code lines each finding is about, as `lines_of` finds them.
    #[must_use]
    pub fn with_lines(mut self, lines_of: impl Fn(&Finding) -> Vec<FindingLine>) -> Self {
        self.lines = self
            .findings
            .iter()
            .map(|finding| (finding.id.clone(), lines_of(finding)))
            .collect();
        self
    }

    pub fn rows(&self) -> Vec<FindingsListRow> {
        self.findings
            .iter()
            .enumerate()
            .map(|(position, finding)| FindingsListRow {
                is_selected: self.selected.contains(&finding.id),
                is_open: self.open.as_ref() == Some(&finding.id),
                is_under_cursor: position == self.cursor,
                lines: self.lines.get(&finding.id).cloned().unwrap_or_default(),
                fix: self.fixes.get(&finding.id).cloned(),
                ..Self::row_of(finding)
            })
            .collect()
    }

    /// A row with nothing of the list's state on it.
    fn row_of(finding: &Finding) -> FindingsListRow {
        FindingsListRow {
            id: finding.id.clone(),
            severity: finding.severity,
            place: place_of(&finding.anchor),
            first_line: finding.body.lines().next().unwrap_or_default().to_owned(),
            body: finding.body.clone(),
            is_selected: false,
            is_open: false,
            is_under_cursor: false,
            lines: Vec::new(),
            fix: None,
            is_outdated: finding.outdated.is_some(),
        }
    }

    /// Nothing to show: no open finding and no dismissed one.
    pub const fn is_empty(&self) -> bool {
        self.findings.is_empty() && self.dismissed.is_empty()
    }

    pub fn toggle_selected(&mut self, id: &FindingId) {
        if !self.selected.remove(id) {
            self.selected.insert(id.clone());
        }
    }

    pub fn toggle_under_cursor(&mut self) {
        if let Some(id) = self.id_under_cursor().cloned() {
            self.toggle_selected(&id);
        } else if let Some(id) = self.dismissed_under_cursor().cloned() {
            self.toggle_dismissed_selected(&id);
        }
    }

    /// Selects every finding, or none when all are.
    pub fn toggle_all(&mut self) {
        if self.all_selected() {
            self.selected.clear();
        } else {
            self.selected = self
                .findings
                .iter()
                .map(|finding| finding.id.clone())
                .collect();
        }
    }

    pub fn all_selected(&self) -> bool {
        !self.findings.is_empty() && self.selected.len() == self.findings.len()
    }

    /// In the list's order.
    pub fn selected(&self) -> Vec<FindingId> {
        self.findings
            .iter()
            .filter(|finding| self.selected.contains(&finding.id))
            .map(|finding| finding.id.clone())
            .collect()
    }

    pub fn cursor_down(&mut self) {
        self.cursor = (self.cursor + 1).min(self.last_row());
    }

    pub const fn cursor_up(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    /// A click on a row puts the cursor there.
    pub fn move_cursor_to(&mut self, id: &FindingId) {
        if let Some(position) = self.findings.iter().position(|finding| finding.id == *id) {
            self.cursor = position;
        }
    }

    /// The open finding the cursor is on; none when it is on a dismissed one.
    pub fn id_under_cursor(&self) -> Option<&FindingId> {
        self.findings.get(self.cursor).map(|finding| &finding.id)
    }

    pub fn dismissed_under_cursor(&self) -> Option<&FindingId> {
        if !self.dismissed_are_shown || self.cursor_is_on_open() {
            return None;
        }
        self.dismissed
            .get(self.cursor - self.findings.len())
            .map(|finding| &finding.id)
    }

    /// The row under the cursor shows its whole finding, or folds again; only one row is open.
    pub fn toggle_open_under_cursor(&mut self) {
        let Some(under_cursor) = self.id_under_cursor().cloned().map(Some) else {
            return;
        };
        self.open = if self.open == under_cursor {
            None
        } else {
            under_cursor
        };
    }

    /// The same list over the findings another list holds now.
    pub fn refreshed_to(&self, newer: &Self) -> Self {
        let mut list = self.refreshed(&newer.findings);
        list.fixes.clone_from(&newer.fixes);
        list.lines.clone_from(&newer.lines);
        list.dismissed.clone_from(&newer.dismissed);
        list.dismissed_selected = self
            .dismissed_selected
            .iter()
            .filter(|id| list.dismissed.iter().any(|finding| finding.id == **id))
            .cloned()
            .collect();
        list.dismissed_are_shown =
            self.dismissed_are_shown || (list.findings.is_empty() && !list.dismissed.is_empty());
        list.cursor = self.cursor.min(list.last_row());
        list
    }

    /// The same list over the findings that are still open: what is ticked stays ticked.
    pub fn refreshed(&self, findings: &[Finding]) -> Self {
        let mut list = Self::of(findings);
        list.selected = self
            .selected
            .iter()
            .filter(|id| list.findings.iter().any(|finding| finding.id == **id))
            .cloned()
            .collect();
        list.cursor = self.cursor.min(list.findings.len().saturating_sub(1));
        list.open = self
            .open
            .clone()
            .filter(|id| list.findings.iter().any(|finding| finding.id == *id));
        list
    }
}

fn place_of(anchor: &Anchor) -> String {
    match anchor {
        Anchor::Change => "the whole change".to_owned(),
        Anchor::File { file } => file.to_string(),
        Anchor::Lines { file, lines, .. } => format!("{file}:{}", lines.start),
    }
}
