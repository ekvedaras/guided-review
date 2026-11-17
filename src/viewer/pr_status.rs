//! The PR status the title bar shows: draft or ready, and the checks rolled up into one word. What
//! the reviewer may do about it is read alongside, so a button is offered only where GitHub would
//! take the change.

use std::sync::Arc;
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

/// How many checks the title bar's list shows before it says how many more there are.
pub const CHECKS_SHOWN: usize = 20;

/// Where one check stands. In the order a list shows them: what needs the reviewer first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CheckState {
    Failing,
    Running,
    Passing,
}

impl CheckState {
    /// From a check run's `status` and, once it completed, its `conclusion`. A skipped or neutral
    /// run did not fail, so it passes; a cancelled or timed out one did not pass.
    pub fn of_check_run(status: &str, conclusion: Option<&str>) -> Self {
        if status != "COMPLETED" {
            return Self::Running;
        }
        match conclusion {
            Some("SUCCESS" | "NEUTRAL" | "SKIPPED") => Self::Passing,
            _ => Self::Failing,
        }
    }

    /// From the `state` of a commit status, which an external service posts.
    pub fn of_status_context(state: &str) -> Self {
        match state {
            "SUCCESS" => Self::Passing,
            "FAILURE" | "ERROR" => Self::Failing,
            _ => Self::Running,
        }
    }
}

/// One check of the PR's head commit: a check run or a commit status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckRun {
    pub name: Arc<str>,
    pub state: CheckState,
    /// Where its run is on GitHub (`detailsUrl` or `targetUrl`), when GitHub says.
    pub page: Option<Arc<str>>,
}

/// The checks the title bar lists, failing first, then running, then passing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckList {
    pub shown: Vec<CheckRun>,
    /// Checks past the cap, which the PR's checks page lists.
    pub hidden: usize,
}

/// What GitHub says of the PR right now, and what the viewer may do about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrStatus {
    pub readiness: Readiness,
    pub checks: Checks,
    pub check_runs: Arc<[CheckRun]>,
    /// GitHub lets only the author and those with write access change it.
    pub viewer_may_change_readiness: bool,
    pub merging: Merging,
}

impl PrStatus {
    /// The chip's words: "Draft" or "Ready". The checks are its colour, its icon and its list.
    pub fn label(&self) -> &'static str {
        match self.readiness {
            Readiness::Draft => "Draft",
            Readiness::Ready => "Ready",
        }
    }

    /// Everything the chip says, in words, for what cannot see its colour: "Ready · checks passing".
    pub fn description(&self) -> String {
        let checks = match self.checks {
            Checks::NoneRun => return self.label().to_owned(),
            Checks::Running => "running",
            Checks::Passing => "passing",
            Checks::Failing => "failing",
        };
        format!("{} · checks {checks}", self.label())
    }

    /// Sorted by state and no further, so the order within a state is GitHub's.
    pub fn check_list(&self) -> CheckList {
        let mut sorted = self.check_runs.to_vec();
        sorted.sort_by_key(|check| check.state);
        let hidden = sorted.len().saturating_sub(CHECKS_SHOWN);
        sorted.truncate(CHECKS_SHOWN);
        CheckList {
            shown: sorted,
            hidden,
        }
    }
}
