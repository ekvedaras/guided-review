use std::time::Instant;

use super::{Review, ReviewCore};
use crate::viewer::code_link::WorkingCopy;
use crate::viewer::comments::CommentResult;
use crate::viewer::hand_offs::{AgentReplied, HandOffId};
use crate::viewer::problems::Read;
use crate::viewer::tour_agent::TourAgentState;
use crate::viewer::work_agenda::{
    AGENT_START_RETRY, AfterLanding, Done, Job, Work, WorkKind, WorkOutcome,
};

impl Review {
    /// The work to start now, each marked as running until its outcome lands. The app runs each
    /// off the UI thread and asks again after every change and when `next_due` comes.
    pub fn due_work(&mut self, now: Instant) -> Vec<Work> {
        let due = self.work_mut().fall_due(now);
        let jobs: Vec<Job> = due
            .into_iter()
            .filter_map(|kind| self.job_of(kind))
            .collect();
        jobs.into_iter()
            .map(|job| self.work_mut().start(job))
            .collect()
    }

    /// When work wanted later falls due; `None` when none is.
    pub fn next_due(&self) -> Option<Instant> {
        self.work.next_due()
    }

    /// The window came to the front, or the review opened: GitHub and the clone are read again,
    /// and what waits to go to GitHub is sent.
    pub fn pull_from_github(&mut self) {
        for kind in WorkKind::PULLS_FROM_GITHUB {
            self.work_mut().want(kind);
        }
    }
}

impl ReviewCore {
    /// `None` when there is nothing of the kind to do now.
    fn job_of(&self, kind: WorkKind) -> Option<Job> {
        let pr = self.pr.clone();
        Some(match kind {
            WorkKind::CheckPrHead => Job::CheckPrHead(pr),
            WorkKind::CheckWorkingCopy => Job::CheckWorkingCopy(self.head.clone()),
            WorkKind::PullPrDescription => Job::PullPrDescription(pr),
            WorkKind::SyncViewed => Job::SyncViewed(self.sync_request()),
            WorkKind::PullThreads => Job::PullThreads {
                pr,
                head: self.head.clone(),
            },
            WorkKind::PullPendingReview => Job::PullPendingReview(pr),
            WorkKind::PullLastReview => Job::PullLastReview(pr),
            WorkKind::SendQueuedComment => Job::SendQueuedComment(self.queued_comment_request()?),
            WorkKind::TakeSeenMarksOff => {
                let requests = self.state.hand_offs().seen_marks_to_take_off();
                if requests.is_empty() {
                    return None;
                }
                Job::TakeSeenMarksOff(requests)
            }
            WorkKind::DeliverHandOff => {
                Job::DeliverHandOff(self.state.hand_offs().delivery_request(&self.pr)?)
            }
        })
    }

    /// Takes in what a `Work` brought back and makes due what follows from it. A save of it that
    /// is refused or fails is not returned: the saved state names it among the problems.
    pub fn land(&mut self, outcome: WorkOutcome) -> AfterLanding {
        let WorkOutcome { kind, ticket, done } = outcome;
        if !self.work.landed(kind, ticket) {
            return AfterLanding::Nothing;
        }
        match done {
            Done::PrHead(Ok(tips)) => self.pr_head_is(tips.head),
            Done::PrHead(Err(error)) => self.could_not_read(Read::PrHead, &error),
            Done::WorkingCopy(is_at) => self.working_copy_is(WorkingCopy::of(is_at)),
            Done::PrDescription(Ok(body)) => {
                self.set_pr_description(&body).ok();
            }
            Done::PrDescription(Err(error)) => self.could_not_read(Read::PrDescription, &error),
            Done::SyncViewed(outcome) => {
                self.apply_sync(outcome).ok();
            }
            Done::Threads(Ok((threads, followed))) => {
                self.set_threads(&threads);
                self.set_followed_lines(followed);
            }
            Done::Threads(Err(error)) => self.threads_not_read(&error),
            Done::PendingReview { exists, authored } => {
                match exists {
                    Ok(exists) => self.pending_review_is(exists),
                    Err(error) => self.could_not_read(Read::PendingReview, &error),
                }
                match authored {
                    Ok(authored) => self.viewer_authorship_is(authored),
                    Err(error) => self.could_not_read(Read::Authorship, &error),
                }
            }
            Done::LastReview(Ok(last)) => self.last_review_is(last),
            Done::LastReview(Err(error)) => self.could_not_read(Read::LastReview, &error),
            Done::QueuedComment(outcome) => {
                if matches!(self.land_comment(outcome), Ok(CommentResult::Sent)) {
                    self.work.want(WorkKind::SendQueuedComment);
                }
            }
            Done::SeenMarksOff(outcomes) => {
                for outcome in outcomes {
                    self.record_hand_offs(|hand_offs| hand_offs.apply_seen_mark_off(outcome))
                        .ok();
                }
            }
            Done::Delivery(outcome) => {
                let after = if outcome.started_an_agent() {
                    AfterLanding::FollowNewTourAgent
                } else {
                    AfterLanding::Nothing
                };
                if outcome.wants_another_try_soon() {
                    self.work
                        .want_in(WorkKind::DeliverHandOff, AGENT_START_RETRY);
                }
                self.record_hand_offs(|hand_offs| hand_offs.apply_delivery(outcome))
                    .ok();
                return after;
            }
        }
        AfterLanding::Nothing
    }

    /// While one is typed into the tour agent; the next waits for it.
    pub fn is_delivering_hand_offs(&self) -> bool {
        self.work.is_running(WorkKind::DeliverHandOff)
    }

    /// What the follow of the tour agent saw. A status moves the hand-offs on and takes the seen
    /// marks off those it ended without answering; any news may let the next hand-off go.
    pub fn tour_agent_is(&mut self, state: &TourAgentState) {
        if let TourAgentState::Agent { status, .. } = state {
            self.record_hand_offs(|hand_offs| hand_offs.agent_status_is(*status))
                .ok();
            self.work.want(WorkKind::TakeSeenMarksOff);
        }
        self.work.want(WorkKind::DeliverHandOff);
    }

    /// The tour agent's reply is on GitHub: the hand-off is answered, and the threads and the
    /// pending review are read again so the reply shows under the asked comment.
    pub fn hand_off_replied(
        &mut self,
        id: HandOffId,
        replied: &AgentReplied,
    ) -> anyhow::Result<()> {
        self.record_hand_offs(|hand_offs| hand_offs.replied(id, replied))??;
        self.work.want(WorkKind::PullThreads);
        self.work.want(WorkKind::PullPendingReview);
        Ok(())
    }

    /// Queues a failed hand-off again and lets the next hand-off go, whether it was queued or not.
    pub fn try_hand_off_again(&mut self, id: HandOffId) -> anyhow::Result<()> {
        self.work.want(WorkKind::DeliverHandOff);
        self.record_hand_offs(|hand_offs| hand_offs.try_again(id))
            .and_then(|tried| tried)
    }
}
