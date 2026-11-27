use super::ReviewCore;
use crate::viewer::cancel::Cancel;
use crate::viewer::problems::Problem;
use crate::viewer::replacing_jobs::{JobProgress, ReplacingJob, StartedJob};

impl ReviewCore {
    /// `None` while another job runs: a reload then waits for it and comes from `due_job`, any
    /// other is refused. Also `None` for a move with no offer, and a drop of a PR's review.
    /// A re-pin, a move and a drop write the state file from another thread, so from their start
    /// nothing this review holds may write it until they failed.
    pub fn start_job(&mut self, job: ReplacingJob) -> Option<StartedJob> {
        if !self.jobs.may_start(job) {
            return None;
        }
        let started = match job {
            ReplacingJob::Repin => StartedJob::Repin,
            ReplacingJob::Refresh => StartedJob::Refresh,
            ReplacingJob::Reload => StartedJob::Reload,
            ReplacingJob::SwitchToGuided => StartedJob::SwitchToGuided,
            ReplacingJob::MoveToPr => {
                StartedJob::MoveToPr(self.branch_link.move_to_start(&self.state)?)
            }
            ReplacingJob::DropReview => StartedJob::DropReview(self.subject.local()?.clone()),
        };
        if matches!(
            job,
            ReplacingJob::Repin | ReplacingJob::MoveToPr | ReplacingJob::DropReview
        ) {
            self.state.writes_elsewhere();
        }
        Some(self.jobs.started(started))
    }

    /// A reload asked for while another job ran, once none runs.
    pub(super) fn due_job(&mut self) -> Option<StartedJob> {
        if !self.jobs.reload_is_due() {
            return None;
        }
        self.start_job(ReplacingJob::Reload)
    }

    /// What the running Refresh's wait looks at.
    pub fn refresh_cancel(&self) -> Cancel {
        self.jobs.refresh_cancel()
    }

    /// The reviewer's Cancel: the Refresh stops waiting for the agent and lands as cancelled, which
    /// frees the slot of the replacing jobs. The agent is not interrupted.
    pub fn cancel_refresh(&self) {
        self.jobs.cancel_refresh();
    }

    /// What `job` did last, for its banner; an activity that arrives after it landed is dropped.
    pub(super) fn job_did(&mut self, job: ReplacingJob, activity: String) {
        self.jobs.did(job, activity);
    }

    /// What `job` is doing or did last, for its banner.
    pub fn job_progress(&self, job: ReplacingJob) -> JobProgress {
        self.jobs.progress(job)
    }

    pub fn job_is_running(&self, job: ReplacingJob) -> bool {
        self.jobs.is_running(job)
    }

    pub(super) fn landed_job(&mut self, job: ReplacingJob) -> Option<StartedJob> {
        self.jobs.landed(job)
    }

    /// The job that wrote the state file from another thread failed, so this review writes it again.
    pub(super) fn writes_here_again(&mut self) {
        self.state.writes_here_again();
    }

    pub(super) fn job_ended_the_review(&mut self) {
        self.jobs.review_ended();
    }

    pub(super) fn job_failed(
        &mut self,
        job: ReplacingJob,
        error: &anyhow::Error,
        problem: Option<Problem>,
    ) {
        self.jobs.failed(job, &format!("{error:#}"));
        if let Some(problem) = problem {
            self.raise_problem(problem);
        }
    }
}
