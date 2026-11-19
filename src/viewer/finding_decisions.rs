//! What the reviewer decided about the findings of an AI review: used in a comment, or dismissed.
//! A decided finding is not shown again; a dismissed one is told to the review agent on a second run.

use serde::{Deserialize, Serialize};

use crate::findings::Finding;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Fate {
    Used,
    Dismissed,
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
    pub fn has_decided(&self, finding: &Finding) -> bool {
        self.0.iter().any(|decision| {
            decision.finding.id == finding.id && decision.finding.body == finding.body
        })
    }

    pub fn record(&mut self, finding: Finding, fate: Fate) {
        self.0.retain(|decision| decision.finding.id != finding.id);
        self.0.push(FindingDecision { finding, fate });
    }

    pub fn dismissed(&self) -> impl Iterator<Item = &Finding> {
        self.0
            .iter()
            .filter(|decision| decision.fate == Fate::Dismissed)
            .map(|decision| &decision.finding)
    }
}
