//! The findings list: one compact row per open finding, for going through many of them fast. It
//! holds what the reviewer has ticked and which row the keys are on, and nothing the findings file holds.

use std::collections::{BTreeMap, BTreeSet};

use super::fix_requests::FixRequests;
use crate::findings::{Anchor, Finding, FindingId, Severity};

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
    /// Where the fix the reviewer asked for is, when one was asked for.
    pub fix: Option<String>,
}

#[derive(Debug, Default, Clone)]
pub struct FindingsList {
    findings: Vec<Finding>,
    selected: BTreeSet<FindingId>,
    cursor: usize,
    open: Option<FindingId>,
    /// What each finding's fix says, for the findings that have one.
    fixes: BTreeMap<FindingId, String>,
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

    pub fn rows(&self) -> Vec<FindingsListRow> {
        self.findings
            .iter()
            .enumerate()
            .map(|(position, finding)| FindingsListRow {
                id: finding.id.clone(),
                severity: finding.severity,
                place: place_of(&finding.anchor),
                first_line: finding.body.lines().next().unwrap_or_default().to_owned(),
                body: finding.body.clone(),
                is_selected: self.selected.contains(&finding.id),
                is_open: self.open.as_ref() == Some(&finding.id),
                is_under_cursor: position == self.cursor,
                fix: self.fixes.get(&finding.id).cloned(),
            })
            .collect()
    }

    pub const fn is_empty(&self) -> bool {
        self.findings.is_empty()
    }

    pub fn toggle_selected(&mut self, id: &FindingId) {
        if !self.selected.remove(id) {
            self.selected.insert(id.clone());
        }
    }

    pub fn toggle_under_cursor(&mut self) {
        if let Some(id) = self.id_under_cursor().cloned() {
            self.toggle_selected(&id);
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
        self.cursor = (self.cursor + 1).min(self.findings.len().saturating_sub(1));
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

    pub fn id_under_cursor(&self) -> Option<&FindingId> {
        self.findings.get(self.cursor).map(|finding| &finding.id)
    }

    /// The row under the cursor shows its whole finding, or folds again; only one row is open.
    pub fn toggle_open_under_cursor(&mut self) {
        let under_cursor = self.id_under_cursor().cloned();
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
        Anchor::Lines { file, lines, .. } => format!("{file}:{}", lines[0]),
    }
}
