//! The PR status the title bar shows: merged or closed, or while open, draft or ready and the checks
//! rolled up into one word. What the reviewer may do about it is read alongside, so a button is
//! offered only where the host would take the change.

use std::sync::Arc;
use std::time::Duration;

/// How often the checks are read again while one is running.
pub const CHECKS_RECHECK: Duration = Duration::from_secs(30);

/// Whether the PR is a draft, which the host does not let anyone merge.
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

/// The checks on the PR's head commit as the host rolls them up. Which check ran is on the host's page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Checks {
    /// The PR's repo runs none.
    NoneRun,
    Running,
    Passing,
    Failing,
}

/// How many checks the title bar's list shows before it says how many more there are.
pub const CHECKS_SHOWN: usize = 20;

/// Where one check stands, in the host's words. In the order a list shows them: what needs the
/// reviewer first. Whether the PR as a whole passes is the host's roll-up, `Checks`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CheckState {
    Failing,
    Running,
    Passing,
    Neutral,
    Skipped,
}

/// One check of the PR's head commit: a check run or a commit status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckRun {
    pub name: Arc<str>,
    pub state: CheckState,
    /// Where its run is on the host (GitHub: `detailsUrl` or `targetUrl`), when the host says.
    pub page: Option<Arc<str>>,
}

/// The checks the title bar lists, failing first, then running, then passing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckList {
    pub shown: Vec<CheckRun>,
    /// Checks past the cap, which the PR's checks page lists.
    pub hidden: usize,
}

/// What the host says of the PR right now. Draft or ready and the checks mean something only while
/// the PR is open; a merged or closed PR is only that. How it would merge is the merging port's
/// (`MergingPort::pull_request_merging`), read only on a host that merges from the app.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrStatus {
    Open(OpenPrStatus),
    Merged,
    Closed,
}

impl PrStatus {
    pub fn open(&self) -> Option<&OpenPrStatus> {
        match self {
            Self::Open(open) => Some(open),
            Self::Merged | Self::Closed => None,
        }
    }

    pub fn open_mut(&mut self) -> Option<&mut OpenPrStatus> {
        match self {
            Self::Open(open) => Some(open),
            Self::Merged | Self::Closed => None,
        }
    }

    /// The chip's words: "Draft" or "Ready" while open, else "Merged" or "Closed".
    pub fn label(&self) -> &'static str {
        match self {
            Self::Open(open) => open.label(),
            Self::Merged => "Merged",
            Self::Closed => "Closed",
        }
    }

    /// Everything the chip says, in words, for what cannot see its colour: "Ready · checks passing".
    pub fn description(&self) -> String {
        match self {
            Self::Open(open) => open.description(),
            Self::Merged | Self::Closed => self.label().to_owned(),
        }
    }
}

/// An open PR's status, and what the viewer may do about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenPrStatus {
    pub readiness: Readiness,
    pub checks: Checks,
    pub check_runs: Arc<[CheckRun]>,
    /// GitHub lets only the author and those with write access change it.
    pub viewer_may_change_readiness: bool,
}

impl OpenPrStatus {
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

    /// Sorted by state and no further, so the order within a state is the host's.
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
