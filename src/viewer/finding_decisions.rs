//! What the reviewer decided about the findings of an AI review: used in a comment, or dismissed.
//! A decided finding is not shown again; a dismissed one is told to the review agent on a second run.

use crate::viewer::kept_values::KeptEntries;
use serde::{Deserialize, Serialize};

use crate::findings::{Finding, FindingId};

use super::threads::ThreadId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Fate {
    Used,
    Dismissed,
    /// Dismissed, then restored: open again, kept from this copy when the findings file no longer holds it.
    Reopened,
    /// The tour agent fixed it in a commit.
    Fixed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FindingDecision {
    pub finding: Finding,
    pub fate: Fate,
    /// The thread the comment that used it became, when GitHub or the local threads showed it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread: Option<ThreadId>,
}

/// Kept whole, with the finding's text, so a later run's report of the same thing is known again.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct FindingDecisions(KeptEntries<FindingDecision>);

impl FindingDecisions {
    /// Entries a newer build wrote in a shape this build does not read; kept in the file as written.
    pub fn unread_entries(&self) -> usize {
        self.0.unread_count()
    }

    pub const fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// A finding counts as decided only when its id and its text are the same: a new run may reuse an id.
    /// A reopened one is open, so it is not decided.
    pub fn has_decided(&self, finding: &Finding) -> bool {
        self.0
            .iter()
            .any(|decision| decision.fate != Fate::Reopened && decision.finding.is_same_as(finding))
    }

    /// The restored findings, in the order they were restored.
    pub fn reopened(&self) -> impl Iterator<Item = &Finding> {
        self.0
            .iter()
            .filter(|decision| decision.fate == Fate::Reopened)
            .map(|decision| &decision.finding)
    }

    /// A re-pin moved the findings: the decided ones keep their new place, for the brief of a later run.
    pub(super) fn follow(&mut self, moved: &[Finding]) {
        for decision in self.0.iter_mut() {
            decision.finding.follow(moved);
        }
    }

    pub fn record(&mut self, finding: Finding, fate: Fate) {
        self.0.retain(|decision| decision.finding.id != finding.id);
        self.0.push(FindingDecision {
            finding,
            fate,
            thread: None,
        });
    }

    /// Used in a comment that became `thread`; `None` while it waits in the queue or when its thread was not read.
    pub fn record_used(&mut self, finding: Finding, thread: Option<ThreadId>) {
        self.0.retain(|decision| decision.finding.id != finding.id);
        self.0.push(FindingDecision {
            finding,
            fate: Fate::Used,
            thread,
        });
    }

    /// The findings whose comment became `thread`, in the order they were used.
    pub fn used_in<'a>(&'a self, thread: &'a ThreadId) -> impl Iterator<Item = &'a Finding> {
        self.0
            .iter()
            .filter(move |decision| {
                decision.fate == Fate::Used && decision.thread.as_ref() == Some(thread)
            })
            .map(|decision| &decision.finding)
    }

    /// The dismissed finding is taken back: it is no longer decided, so it is not told to a later run,
    /// and its copy stays to open it again where the findings file no longer holds it.
    pub fn restore(&mut self, id: &FindingId) -> Option<Finding> {
        let decision = self
            .0
            .iter_mut()
            .find(|decision| decision.fate == Fate::Dismissed && decision.finding.id == *id)?;
        decision.fate = Fate::Reopened;
        Some(decision.finding.clone())
    }

    /// A snapshot scene's findings used in `thread`, taken back when the scene ends.
    #[cfg(feature = "snapshots")]
    pub(super) fn forget_used_in(&mut self, thread: &ThreadId) {
        self.0
            .retain(|decision| decision.thread.as_ref() != Some(thread));
    }

    pub fn dismissed(&self) -> impl Iterator<Item = &Finding> {
        self.0
            .iter()
            .filter(|decision| decision.fate == Fate::Dismissed)
            .map(|decision| &decision.finding)
    }
}
