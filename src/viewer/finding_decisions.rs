//! What the reviewer decided about the findings of an AI review: used in a comment, or dismissed.
//! A decided finding is not shown again; a dismissed one is told to the review agent on a second run.

use serde::{Deserialize, Serialize};

use crate::findings::{Finding, FindingId};

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
}

/// Kept whole, with the finding's text, so a later run's report of the same thing is known again.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct FindingDecisions(Vec<FindingDecision>);

impl FindingDecisions {
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
        for decision in &mut self.0 {
            decision.finding.follow(moved);
        }
    }

    pub fn record(&mut self, finding: Finding, fate: Fate) {
        self.0.retain(|decision| decision.finding.id != finding.id);
        self.0.push(FindingDecision { finding, fate });
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

    pub fn dismissed(&self) -> impl Iterator<Item = &Finding> {
        self.0
            .iter()
            .filter(|decision| decision.fate == Fate::Dismissed)
            .map(|decision| &decision.finding)
    }
}
