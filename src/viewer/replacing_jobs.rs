//! The jobs that replace the review on screen with one built again from disk, or end it: Re-pin,
//! Refresh, reading the tour again, the switch to the guided tour, Move to PR and Drop review. Each
//! starts from the files as they are and builds or ends the review from them, so a second one
//! running beside it would build on what the first is changing. At most one runs; the app runs
//! each off the UI thread and lands its outcome here, which says what follows.

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::pull_request::PullRequestUrl;
use crate::review_subject::{LocalChangeSet, ReviewSubject};

use super::move_offer::StartedMove;
use super::new_commits::RepinnedTour;
use super::review::Screen;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ReplacingJob {
    /// Update tour, or a local review's Update: the re-pin.
    Repin,
    Refresh,
    /// The tour file read again, when `open` names the subject shown or a clone was made.
    Reload,
    SwitchToGuided,
    MoveToPr,
    DropReview,
}

/// A job the review let start: what the app needs to run it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartedJob {
    Repin,
    Refresh,
    Reload,
    SwitchToGuided,
    MoveToPr(StartedMove),
    DropReview(LocalChangeSet),
}

impl StartedJob {
    pub const fn job(&self) -> ReplacingJob {
        match self {
            Self::Repin => ReplacingJob::Repin,
            Self::Refresh => ReplacingJob::Refresh,
            Self::Reload => ReplacingJob::Reload,
            Self::SwitchToGuided => ReplacingJob::SwitchToGuided,
            Self::MoveToPr(_) => ReplacingJob::MoveToPr,
            Self::DropReview(_) => ReplacingJob::DropReview,
        }
    }
}

/// What a job's run gave.
pub enum JobOutcome {
    Repin(anyhow::Result<RepinnedTour>),
    Refresh(anyhow::Result<RepinnedTour>),
    Reload(anyhow::Result<RepinnedTour>),
    SwitchToGuided(anyhow::Result<RepinnedTour>),
    MoveToPr(anyhow::Result<()>),
    DropReview(anyhow::Result<()>),
}

impl JobOutcome {
    pub const fn job(&self) -> ReplacingJob {
        match self {
            Self::Repin(_) => ReplacingJob::Repin,
            Self::Refresh(_) => ReplacingJob::Refresh,
            Self::Reload(_) => ReplacingJob::Reload,
            Self::SwitchToGuided(_) => ReplacingJob::SwitchToGuided,
            Self::MoveToPr(_) => ReplacingJob::MoveToPr,
            Self::DropReview(_) => ReplacingJob::DropReview,
        }
    }
}

/// What the app does once a job landed.
pub enum AfterJob {
    /// The outcome was not of the job running, so it changes nothing.
    Nothing,
    /// The review the job built takes this one's place, on `screen`.
    Reopen {
        replacement: Box<RepinnedTour>,
        screen: Screen,
    },
    /// The local review moved to its PR and is gone: the PR is shown in its place.
    ShowPr {
        moved_away: ReviewSubject,
        pr: PullRequestUrl,
    },
    /// The local review of the PR's branch moved here and is gone: the PR's review opens again from
    /// disk, since this copy's state is stale once the move saved the PR's.
    DropAndReopen(ReviewSubject),
    /// The review is gone from disk: the switcher takes its place.
    Dropped(ReviewSubject),
    /// The job failed and the review stays. Its banner or its problems say why, else `toast` does.
    Stays { toast: Option<String> },
}

/// What a job's banner or button shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JobProgress {
    Idle,
    /// `activity` is what the job said it did last: the tool claude used during a Refresh.
    Running {
        activity: Option<Arc<str>>,
    },
    /// Until the job starts again.
    Failed(Arc<str>),
}

/// Which replacing job runs, and why each one failed last.
#[derive(Default)]
pub struct ReplacingJobs {
    running: Option<StartedJob>,
    activity: Option<Arc<str>>,
    /// A reload asked for while another job ran, which starts once it landed.
    reload_wanted: bool,
    failures: BTreeMap<ReplacingJob, Arc<str>>,
}

impl ReplacingJobs {
    pub fn progress(&self, job: ReplacingJob) -> JobProgress {
        if self.is_running(job) {
            JobProgress::Running {
                activity: self.activity.clone(),
            }
        } else if let Some(reason) = self.failures.get(&job) {
            JobProgress::Failed(Arc::clone(reason))
        } else {
            JobProgress::Idle
        }
    }

    pub fn is_running(&self, job: ReplacingJob) -> bool {
        self.running
            .as_ref()
            .is_some_and(|running| running.job() == job)
    }

    /// `false` while another job runs; a reload is then wanted once it landed.
    pub(super) fn may_start(&mut self, job: ReplacingJob) -> bool {
        if self.running.is_none() {
            return true;
        }
        if job == ReplacingJob::Reload {
            self.reload_wanted = true;
        }
        false
    }

    pub(super) fn started(&mut self, started: StartedJob) -> StartedJob {
        let job = started.job();
        self.failures.remove(&job);
        if job == ReplacingJob::Reload {
            self.reload_wanted = false;
        }
        self.activity = None;
        self.running = Some(started.clone());
        started
    }

    pub(super) const fn reload_is_due(&self) -> bool {
        self.running.is_none() && self.reload_wanted
    }

    pub(super) fn did(&mut self, job: ReplacingJob, activity: String) {
        if self.is_running(job) {
            self.activity = Some(activity.into());
        }
    }

    /// The job that ran, when `job` is the one running; it runs no more.
    pub(super) fn landed(&mut self, job: ReplacingJob) -> Option<StartedJob> {
        if !self.is_running(job) {
            return None;
        }
        self.activity = None;
        self.running.take()
    }

    pub(super) fn failed(&mut self, job: ReplacingJob, reason: &str) {
        self.failures.insert(job, reason.into());
    }

    /// The review is gone, so nothing waits to read its tour again.
    pub(super) const fn review_ended(&mut self) {
        self.reload_wanted = false;
    }
}
