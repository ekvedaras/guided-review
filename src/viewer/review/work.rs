use std::time::Instant;

use super::ReviewCore;
use crate::viewer::code_link::WorkingCopy;
use crate::viewer::comments::CommentResult;
use crate::viewer::hand_offs::{AgentReplied, HandOffButton, HandOffId};
use crate::viewer::host_outage::Read;
use crate::viewer::problems::{Problem, ProblemKind};
use crate::viewer::related_issues::RelatedIssues;
use crate::viewer::tour_agent::{TourAgentState, TourKind};
use crate::viewer::work_agenda::{
    AGENT_LINK_RECHECK, AGENT_START_RETRY, AfterLanding, Done, Job, WORKING_TREE_RECHECK, Work,
    WorkKind, WorkOutcome,
};

impl ReviewCore {
    /// The work to start now, each marked as running until its outcome lands. The app runs each
    /// off the UI thread and asks again after every change and when `next_due` comes.
    pub(super) fn due_work(&mut self, now: Instant) -> Vec<Work> {
        let due = self.work.fall_due(now);
        #[expect(
            clippy::needless_collect,
            reason = "job_of reads the whole review, and start changes its agenda"
        )]
        let jobs: Vec<Job> = due
            .into_iter()
            .filter_map(|kind| self.job_of(kind))
            .collect();
        jobs.into_iter().map(|job| self.work.start(job)).collect()
    }

    /// When work wanted later falls due; `None` when none is.
    pub fn next_due(&self) -> Option<Instant> {
        self.work.next_due()
    }

    /// The window came to the front, or the review opened: the host and the clone are read again,
    /// and what waits to go to the host is sent.
    pub(super) fn pull_from_host(&mut self) {
        for kind in WorkKind::READ_AGAIN_ON_FOCUS {
            self.work.want(kind);
        }
    }

    /// `None` when there is nothing of the kind to do now. Work with the host is never done for a local
    /// review, which has no PR.
    fn job_of(&self, kind: WorkKind) -> Option<Job> {
        let pr = || self.subject.pull_request().cloned();
        Some(match kind {
            WorkKind::CheckPrHead => Job::CheckPrHead(pr()?),
            WorkKind::CheckWorkingTree => Job::CheckWorkingTree {
                change_set: self.subject.local()?.clone(),
                snapshot: self.head.clone(),
            },
            // A local review learns it from the look at its folder.
            WorkKind::CheckWorkingCopy => {
                pr()?;
                Job::CheckWorkingCopy(self.head.clone())
            }
            WorkKind::LookUpPullOfBranch => Job::LookUpPullOfBranch {
                repo: self.local_facts.as_ref()?.origin.clone()?,
                branch: self.subject.local()?.branch().clone(),
            },
            WorkKind::LookUpHeadBranch => {
                Job::LookUpHeadBranch(self.branch_link.head_branch_to_look_up()?.clone())
            }
            WorkKind::PullPrDescription => Job::PullPrDescription(pr()?),
            WorkKind::PullRelatedIssues => Job::PullRelatedIssues(pr()?),
            WorkKind::PullPrOrigin => Job::PullPrOrigin(pr()?),
            WorkKind::PullIssueDetails => {
                let issues = self.related_issues.details_to_read();
                if issues.is_empty() {
                    return None;
                }
                Job::PullIssueDetails(issues)
            }
            WorkKind::SyncViewed => Job::SyncViewed(self.sync_request()?),
            WorkKind::PullThreads => Job::PullThreads {
                home: self.thread_home(),
                head: self.head.clone(),
            },
            WorkKind::PullPendingReview => Job::PullPendingReview(pr()?),
            WorkKind::PullReviews => Job::PullReviews(pr()?),
            WorkKind::PullPrStatus => Job::PullPrStatus(pr()?),
            WorkKind::SendQueuedComment => Job::SendQueuedComment(self.queued_comment_request()?),
            WorkKind::ChangeReviewingMark => Job::ChangeReviewingMark {
                pr: pr()?,
                change: self.reviewing_mark.change_to_send()?,
            },
            WorkKind::ChangePrReadiness => Job::ChangePrReadiness {
                pr: pr()?,
                readiness: self.readiness_to_send()?,
            },
            WorkKind::ChangeMerging => Job::ChangeMerging {
                pr: pr()?,
                change: self.merging_to_send()?,
            },
            WorkKind::TakeSeenMarksOff => {
                let requests = self.state.hand_offs().seen_marks_to_take_off();
                if requests.is_empty() {
                    return None;
                }
                Job::TakeSeenMarksOff(requests)
            }
            WorkKind::DeliverHandOff => {
                match self.state.hand_offs().delivery_request(&self.subject) {
                    Some(request) => Job::DeliverHandOff(request),
                    None => Job::DeliverFixRequests(self.state.fix_requests().delivery_request(
                        &self.subject,
                        if self.tour.guided().is_some() {
                            TourKind::Guided
                        } else {
                            TourKind::Basic
                        },
                    )?),
                }
            }
            WorkKind::CheckAgentLink => {
                if !self.agent_link_is_off {
                    return None;
                }
                Job::CheckAgentLink(self.subject.clone())
            }
        })
    }

    /// Reads the details of the issues again when the change shows others.
    pub(super) fn change_related_issues(&mut self, change: impl FnOnce(&mut RelatedIssues)) {
        let before = self.related_issues.details_to_read();
        change(&mut self.related_issues);
        if self.related_issues.details_to_read() != before {
            self.work.want(WorkKind::PullIssueDetails);
        }
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
            Done::WorkingTree(on_disk) => {
                match on_disk {
                    Ok(on_disk) => {
                        self.clear_problem(ProblemKind::FolderNotLookedAt);
                        self.working_tree_is(on_disk);
                    }
                    Err(error) => self.raise_problem(Problem::FolderNotLookedAt(error)),
                }
                self.work
                    .want_in(WorkKind::CheckWorkingTree, WORKING_TREE_RECHECK);
            }
            Done::WorkingCopy(is_at) => self.working_copy_is(WorkingCopy::of(is_at)),
            Done::PullOfBranch(found) => {
                if let Some(wait) = self.branch_link.pull_looked_up(found) {
                    self.work.want_in(WorkKind::LookUpPullOfBranch, wait);
                }
            }
            Done::HeadBranch(branch) => {
                if let Some(wait) = self.branch_link.head_branch_looked_up(branch) {
                    self.work.want_in(WorkKind::LookUpHeadBranch, wait);
                }
            }
            Done::PrDescription(Ok(body)) => {
                self.set_pr_description(&body).ok();
            }
            Done::PrDescription(Err(error)) => self.could_not_read(Read::PrDescription, &error),
            // The issue button is a shortcut, not something to warn of: a failed read keeps the last answer.
            Done::RelatedIssues(Ok(issues)) => {
                self.change_related_issues(|related| related.host_says(issues));
            }
            // Like the issue button, the origin is no cause for a warning: a failed read keeps the last answer.
            Done::PrOrigin(Ok(origin)) => self.pr_origin = Some(origin),
            Done::IssueDetails(Ok(read)) => self.related_issues.details_say(read),
            Done::RelatedIssues(Err(_)) | Done::PrOrigin(Err(_)) | Done::IssueDetails(Err(_)) => {}
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
            Done::Reviews(Ok(reviews)) => self.reviews_are(reviews),
            Done::Reviews(Err(error)) => self.could_not_read(Read::LastReview, &error),
            Done::PrStatus {
                status: Ok(status),
                merging,
            } => {
                self.pr_status_is(status);
                match merging {
                    Some(Ok(merging)) => self.pr_merging_is(merging),
                    Some(Err(error)) => self.could_not_read(Read::PrStatus, &error),
                    None => {}
                }
            }
            Done::PrStatus {
                status: Err(error), ..
            } => self.could_not_read(Read::PrStatus, &error),
            Done::ReviewingMark { change, result } => self.reviewing_mark_landed(change, result),
            Done::PrReadiness { readiness, result } => self.readiness_landed(readiness, result),
            Done::Merging { change, result } => self.merging_landed(change, result),
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
            Done::FixDelivery(outcome) => {
                let after = if outcome.started_an_agent() {
                    AfterLanding::FollowNewTourAgent
                } else {
                    AfterLanding::Nothing
                };
                self.record_reporting("the fix requests", |state| {
                    state.fix_requests_mut().apply_delivery(outcome);
                });
                self.place_threads();
                return after;
            }
            Done::AgentLink(state) => {
                if state.can_retry() {
                    self.work
                        .want_in(WorkKind::CheckAgentLink, AGENT_LINK_RECHECK);
                } else {
                    self.tour_agent_is(&state);
                }
                return AfterLanding::TourAgentIs(state);
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
        self.agent_link_is_off = state.can_retry();
        if self.agent_link_is_off {
            self.work
                .want_in(WorkKind::CheckAgentLink, AGENT_LINK_RECHECK);
        }
        if let TourAgentState::Agent { status, .. } = state {
            self.record_hand_offs(|hand_offs| hand_offs.agent_status_is(*status))
                .ok();
            if !self.state.fix_requests().is_empty() {
                self.record_reporting("the fix requests", |state| {
                    state.fix_requests_mut().agent_status_is(*status);
                });
                self.place_threads();
            }
            self.work.want(WorkKind::TakeSeenMarksOff);
        }
        self.work.want(WorkKind::DeliverHandOff);
    }

    /// Retry beside the message of an off link: looks at herdr now, not when the timer says.
    pub fn retry_agent_link(&mut self) {
        self.work.want(WorkKind::CheckAgentLink);
    }

    /// The tour agent's reply is on the host: the hand-off is answered, and the threads and the
    /// pending review are read again so the reply shows under the asked comment. While a re-pin runs it
    /// is kept for the re-pinned review, which saves it: the agent's reply is on the host either way.
    pub fn hand_off_replied(
        &mut self,
        id: HandOffId,
        replied: &AgentReplied,
    ) -> anyhow::Result<()> {
        if self.state.is_repinning() {
            self.state
                .keep_until_repinned(|state| state.hand_offs_mut().replied(id, replied))?;
            self.replies_after_repin.push((id, replied.clone()));
        } else {
            self.record_hand_offs(|hand_offs| hand_offs.replied(id, replied))??;
        }
        self.work.want(WorkKind::PullThreads);
        self.work.want(WorkKind::PullPendingReview);
        Ok(())
    }

    pub fn press_hand_off_button(&mut self, button: HandOffButton) -> anyhow::Result<()> {
        match button {
            HandOffButton::TryAgain(id) => self.try_hand_off_again(id),
            HandOffButton::SendAnyway(id) => {
                self.work.want(WorkKind::DeliverHandOff);
                self.record_hand_offs(|hand_offs| hand_offs.send_anyway(id))
                    .and_then(|sent| sent)
            }
        }
    }

    /// Queues a failed hand-off again and lets the next hand-off go, whether it was queued or not.
    pub fn try_hand_off_again(&mut self, id: HandOffId) -> anyhow::Result<()> {
        self.work.want(WorkKind::DeliverHandOff);
        self.record_hand_offs(|hand_offs| hand_offs.try_again(id))
            .and_then(|tried| tried)
    }
}
