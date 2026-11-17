//! The PR status the title bar shows: draft or ready, and the checks rolled up into one word. What
//! the reviewer may do about it is read alongside, so a button is offered only where GitHub would
//! take the change.

use std::time::Duration;

use super::merging::Merging;

/// How often the checks are read again while one is running.
pub const CHECKS_RECHECK: Duration = Duration::from_secs(30);

/// Whether the PR is a draft, which GitHub does not let anyone merge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Readiness {
    Draft,
    Ready,
}

impl Readiness {
    pub fn other(self) -> Self {
        match self {
            Self::Draft => Self::Ready,
            Self::Ready => Self::Draft,
        }
    }
}

/// The checks on the PR's head commit as GitHub rolls them up. Which check ran is on GitHub's page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Checks {
    /// The PR's repo runs none.
    NoneRun,
    Running,
    Passing,
    Failing,
}

impl Checks {
    /// From the `state` of a commit's `statusCheckRollup`; `None` when the commit has none.
    pub fn from_github_state(state: Option<&str>) -> Self {
        match state {
            None => Self::NoneRun,
            Some("SUCCESS") => Self::Passing,
            Some("FAILURE" | "ERROR") => Self::Failing,
            Some(_) => Self::Running,
        }
    }
}

/// What GitHub says of the PR right now, and what the viewer may do about it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrStatus {
    pub readiness: Readiness,
    pub checks: Checks,
    /// GitHub lets only the author and those with write access change it.
    pub viewer_may_change_readiness: bool,
    pub merging: Merging,
}

impl PrStatus {
    /// The words next to the PR number: "Draft", "Ready · checks passing".
    pub fn label(self) -> String {
        let readiness = match self.readiness {
            Readiness::Draft => "Draft",
            Readiness::Ready => "Ready",
        };
        let checks = match self.checks {
            Checks::NoneRun => return readiness.to_owned(),
            Checks::Running => "running",
            Checks::Passing => "passing",
            Checks::Failing => "failing",
        };
        format!("{readiness} · checks {checks}")
    }
}
