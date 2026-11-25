use super::Review;
use crate::viewer::move_offer::MoveFrom;
use crate::viewer::problems::Problem;
use crate::viewer::replacing_jobs::{
    AfterJob, JobOutcome, ReplacingJob, ReplacingJobs, StartedJob,
};
use crate::viewer::screen::Screen;

impl Review {
    /// `None` while another job runs: a reload then waits for it and comes from `due_job`, any
    /// other is refused. Also `None` for a move with no offer, and a drop of a PR's review.
    /// A re-pin, a move and a drop write the state file from another thread, so from their start
    /// nothing this review holds may write it until they failed.
    pub fn start_job(&mut self, job: ReplacingJob) -> Option<StartedJob> {
        if !self.jobs_mut().may_start(job) {
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
        Some(self.jobs_mut().started(started))
    }

    /// A reload asked for while another job ran, once none runs.
    pub fn due_job(&mut self) -> Option<StartedJob> {
        if !self.jobs.reload_is_due() {
            return None;
        }
        self.start_job(ReplacingJob::Reload)
    }

    pub fn land_job(&mut self, outcome: JobOutcome) -> AfterJob {
        let Some(started) = self.jobs_mut().landed(outcome.job()) else {
            return AfterJob::Nothing;
        };
        match outcome {
            JobOutcome::Repin(Ok(replacement))
            | JobOutcome::Refresh(Ok(replacement))
            | JobOutcome::Reload(Ok(replacement)) => AfterJob::Reopen {
                replacement: Box::new(replacement),
                screen: self.screen.clone(),
            },
            JobOutcome::SwitchToGuided(Ok(replacement)) => AfterJob::Reopen {
                replacement: Box::new(replacement),
                screen: Screen::Summary,
            },
            JobOutcome::Repin(Err(error)) => {
                self.state.writes_here_again();
                self.job_failed(ReplacingJob::Repin, &error, None)
            }
            JobOutcome::Refresh(Err(error)) => self.job_failed(
                ReplacingJob::Refresh,
                &error,
                Some(Problem::TourNotRefreshed(format!("{error:#}"))),
            ),
            JobOutcome::Reload(Err(error)) => self.job_failed(
                ReplacingJob::Reload,
                &error,
                Some(Problem::TourNotReloaded(format!("{error:#}"))),
            ),
            JobOutcome::SwitchToGuided(Err(error)) => {
                self.job_failed(ReplacingJob::SwitchToGuided, &error, None);
                AfterJob::Stays {
                    toast: Some(format!("Could not switch to the guided tour: {error:#}")),
                }
            }
            JobOutcome::MoveToPr(moved) => {
                let StartedJob::MoveToPr(started) = started else {
                    return AfterJob::Nothing;
                };
                match moved {
                    Ok(()) => {
                        self.jobs_mut().review_ended();
                        match started.from {
                            MoveFrom::TheLocalReview => AfterJob::ShowPr {
                                moved_away: self.subject.clone(),
                                pr: started.offer.pr,
                            },
                            MoveFrom::ThePr => AfterJob::DropAndReopen(started.change_set.into()),
                        }
                    }
                    Err(error) => {
                        self.state.writes_here_again();
                        self.job_failed(ReplacingJob::MoveToPr, &error, None);
                        AfterJob::Stays {
                            toast: Some(started.failure_text(&error)),
                        }
                    }
                }
            }
            JobOutcome::DropReview(Ok(())) => {
                self.jobs_mut().review_ended();
                AfterJob::Dropped(self.subject.clone())
            }
            JobOutcome::DropReview(Err(error)) => {
                self.state.writes_here_again();
                self.job_failed(
                    ReplacingJob::DropReview,
                    &error,
                    Some(Problem::ReviewNotDropped(format!("{error:#}"))),
                )
            }
        }
    }

    /// What `job` did last, for its banner; an activity that arrives after it landed is dropped.
    pub fn job_did(&mut self, job: ReplacingJob, activity: String) {
        self.jobs_mut().did(job, activity);
    }

    pub fn jobs(&self) -> &ReplacingJobs {
        &self.jobs
    }

    fn job_failed(
        &mut self,
        job: ReplacingJob,
        error: &anyhow::Error,
        problem: Option<Problem>,
    ) -> AfterJob {
        self.jobs_mut().failed(job, &format!("{error:#}"));
        if let Some(problem) = problem {
            self.raise_problem(problem);
        }
        AfterJob::Stays { toast: None }
    }
}
