//! The work a review wants done off the UI thread: which of it is due, which runs, and what is
//! dropped. The review decides what each outcome makes due next; the app only runs each `Work`
//! with the ports and lands its outcome, and tests drive the same loop in line.

use crate::viewer::host_outage::Affected;
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use crate::code_source::CodeSource;
use crate::issue::IssueUrl;
use crate::local_work_tree::OnDisk;
use crate::pull_request::{ChangeTips, HostedRepo, PullRequestUrl, Sha};
use crate::review_subject::{BranchName, LocalChangeSet, ReviewSubject};

use super::capability::Capability;
use super::comment_send::{CommentOutcome, CommentRequest};
use super::fix_requests::{FixDeliveryOutcome, FixDeliveryRequest};
use super::hand_offs::{DeliveryOutcome, DeliveryRequest, SeenMarkOff, SeenMarkOffOutcome};
use super::local_thread_store::ThreadHome;
use super::merging::{MergeChange, Merging};
use super::outdated_lines::{FollowedLines, follow_outdated};
use super::ports::{CodeHost, HostError, PullOfBranch};
use super::pr_origin::PullRequestOrigin;
use super::pr_reviewers::PullRequestReviews;
use super::pr_status::{PrStatus, Readiness};
use super::reactions::ReactionChange;
use super::related_issues::{IssueDetails, PullRequestIssues};
use super::review_thread::{ReviewThread, ThreadId};
use super::reviewing_mark::REVIEWING_MARK;
use super::tour_agent::{TourAgentState, TourAgents};
use super::viewed_sync::{SyncOutcome, SyncRequest};

/// A tick's sync waits this long for the next tick, so a burst of ticks is one round trip.
pub const PUSH_DEBOUNCE: Duration = Duration::from_secs(1);
/// How long to wait before handing off again to an agent whose screen had not settled since it started.
pub const AGENT_START_RETRY: Duration = Duration::from_secs(3);
/// How long to wait before looking again at a link to herdr that is off.
pub const AGENT_LINK_RECHECK: Duration = Duration::from_secs(30);
/// How often a local review's folder is looked at again while the review is open.
pub const WORKING_TREE_RECHECK: Duration = Duration::from_secs(30);
/// How long to wait before looking again for the PR of a local review's branch after GitHub did not answer.
pub const PULL_OF_BRANCH_RETRY: Duration = Duration::from_secs(60);

/// The kinds of work, in the order due work of several kinds starts in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum WorkKind {
    CheckPrHead,
    CheckWorkingTree,
    CheckWorkingCopy,
    LookUpPullOfBranch,
    LookUpHeadBranch,
    PullPrDescription,
    PullRelatedIssues,
    PullPrOrigin,
    PullIssueDetails,
    SyncViewed,
    PullThreads,
    PullPendingReview,
    PullReviews,
    PullPrStatus,
    SendQueuedComment,
    ChangeReviewingMark,
    ChangePrReadiness,
    ChangeMerging,
    TakeSeenMarksOff,
    DeliverHandOff,
    CheckAgentLink,
}

/// What happens to work of a kind that falls due while one of its kind runs.
enum WhileRunning {
    /// It runs once more after the running one lands, so nothing asked for meanwhile is missed.
    RunsAgainAfter,
    /// It is dropped: the running one does what it would do.
    IsDropped,
    /// It starts at once, and the running one's outcome is dropped when it lands, being older.
    Replaces,
}

impl WorkKind {
    /// What a window coming to the front reads and sends again. A review turns each into work only
    /// when it has a use for it: a PR's GitHub work, a local review's look at its folder.
    /// A PR's head branch is not among them: it never changes, so it is looked up once per open.
    pub(super) const READ_AGAIN_ON_FOCUS: [Self; 15] = [
        Self::CheckPrHead,
        Self::CheckWorkingTree,
        Self::CheckWorkingCopy,
        Self::LookUpPullOfBranch,
        Self::PullPrDescription,
        Self::PullRelatedIssues,
        Self::PullPrOrigin,
        Self::PullIssueDetails,
        Self::SyncViewed,
        Self::PullThreads,
        Self::PullPendingReview,
        Self::PullReviews,
        Self::PullPrStatus,
        Self::SendQueuedComment,
        Self::ChangeReviewingMark,
    ];

    fn while_running(self) -> WhileRunning {
        match self {
            Self::SyncViewed | Self::ChangeReviewingMark => WhileRunning::RunsAgainAfter,
            Self::SendQueuedComment
            | Self::TakeSeenMarksOff
            | Self::DeliverHandOff
            | Self::ChangePrReadiness
            | Self::ChangeMerging
            | Self::CheckAgentLink => WhileRunning::IsDropped,
            Self::CheckPrHead
            | Self::CheckWorkingTree
            | Self::CheckWorkingCopy
            | Self::LookUpPullOfBranch
            | Self::LookUpHeadBranch
            | Self::PullPrDescription
            | Self::PullRelatedIssues
            | Self::PullPrOrigin
            | Self::PullIssueDetails
            | Self::PullThreads
            | Self::PullPendingReview
            | Self::PullReviews
            | Self::PullPrStatus => WhileRunning::Replaces,
        }
    }
}

/// Names one started work, so an outcome that another of its kind replaced is known when it lands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Ticket(u64);

#[derive(Debug, Clone, Copy)]
enum Later {
    /// Not placed in time yet: the delay counts from when the agenda is next asked.
    In(Duration),
    At(Instant),
}

#[derive(Debug, Default)]
struct Slot {
    wanted_now: bool,
    /// One per kind: wanting it later again puts it off.
    later: Option<Later>,
    running: Option<Ticket>,
    again_after_running: bool,
}

/// Which work is wanted, when, and which runs. It knows nothing of the review; the review turns
/// each due kind into a `Work` with what it holds now.
#[derive(Debug, Default)]
pub(super) struct WorkAgenda {
    slots: BTreeMap<WorkKind, Slot>,
    issued: u64,
}

impl WorkAgenda {
    pub(super) fn want(&mut self, kind: WorkKind) {
        self.slot(kind).wanted_now = true;
    }

    /// The delay counts from the next `fall_due`, which the app calls right after every change.
    pub(super) fn want_in(&mut self, kind: WorkKind, delay: Duration) {
        self.slot(kind).later = Some(Later::In(delay));
    }

    /// The kinds to start now, in their order. A kind that is due while one of it runs follows its
    /// `WhileRunning` rule.
    pub(super) fn fall_due(&mut self, now: Instant) -> Vec<WorkKind> {
        let mut due = Vec::new();
        for (kind, slot) in &mut self.slots {
            if let Some(Later::In(delay)) = slot.later {
                slot.later = Some(Later::At(now + delay));
            }
            let later_is_due = matches!(slot.later, Some(Later::At(at)) if at <= now);
            if later_is_due {
                slot.later = None;
            }
            if !std::mem::take(&mut slot.wanted_now) && !later_is_due {
                continue;
            }
            match (slot.running, kind.while_running()) {
                (None, _) | (Some(_), WhileRunning::Replaces) => due.push(*kind),
                (Some(_), WhileRunning::RunsAgainAfter) => slot.again_after_running = true,
                (Some(_), WhileRunning::IsDropped) => {}
            }
        }
        due
    }

    pub(super) fn start(&mut self, job: Job) -> Work {
        self.issued += 1;
        let ticket = Ticket(self.issued);
        self.slot(job.kind()).running = Some(ticket);
        Work { ticket, job }
    }

    /// `false` for the outcome of work another of its kind replaced: it is older, so it is dropped.
    pub(super) fn landed(&mut self, kind: WorkKind, ticket: Ticket) -> bool {
        let slot = self.slot(kind);
        if slot.running != Some(ticket) {
            return false;
        }
        slot.running = None;
        if std::mem::take(&mut slot.again_after_running) {
            slot.wanted_now = true;
        }
        true
    }

    pub(super) fn is_running(&self, kind: WorkKind) -> bool {
        self.slots
            .get(&kind)
            .is_some_and(|slot| slot.running.is_some())
    }

    /// When the earliest work wanted later falls due. Work wanted later since the last `fall_due`
    /// is not placed in time yet, so it is not counted.
    pub(super) fn next_due(&self) -> Option<Instant> {
        self.slots
            .values()
            .filter_map(|slot| match slot.later {
                Some(Later::At(at)) => Some(at),
                Some(Later::In(_)) | None => None,
            })
            .min()
    }

    fn slot(&mut self, kind: WorkKind) -> &mut Slot {
        self.slots.entry(kind).or_default()
    }
}

/// What a `Work` runs with. Borrowed, so the app can lend its shared ports and tests their fakes.
pub struct WorkPorts<'a> {
    pub host: &'a dyn CodeHost,
    pub source: &'a dyn CodeSource,
    pub agents: &'a TourAgents,
}

/// One piece of due work with all it needs, owned so it can run on a background thread.
pub struct Work {
    ticket: Ticket,
    job: Job,
}

pub(super) enum Job {
    CheckPrHead(PullRequestUrl),
    CheckWorkingTree {
        change_set: LocalChangeSet,
        snapshot: Sha,
    },
    CheckWorkingCopy(Sha),
    LookUpPullOfBranch {
        repo: HostedRepo,
        branch: BranchName,
    },
    LookUpHeadBranch(PullRequestUrl),
    PullPrDescription(PullRequestUrl),
    PullRelatedIssues(PullRequestUrl),
    PullPrOrigin(PullRequestUrl),
    PullIssueDetails(Vec<IssueUrl>),
    SyncViewed(SyncRequest),
    PullThreads {
        home: ThreadHome,
        head: Sha,
    },
    PullPendingReview(PullRequestUrl),
    PullReviews(PullRequestUrl),
    PullPrStatus(PullRequestUrl),
    SendQueuedComment(CommentRequest),
    ChangeReviewingMark {
        pr: PullRequestUrl,
        change: ReactionChange,
    },
    ChangePrReadiness {
        pr: PullRequestUrl,
        readiness: Readiness,
    },
    ChangeMerging {
        pr: PullRequestUrl,
        change: MergeChange,
    },
    TakeSeenMarksOff(Vec<SeenMarkOff>),
    DeliverHandOff(DeliveryRequest),
    /// Shares the hand-offs' kind: one is typed into the tour agent at a time.
    DeliverFixRequests(FixDeliveryRequest),
    CheckAgentLink(ReviewSubject),
}

impl Job {
    fn kind(&self) -> WorkKind {
        match self {
            Self::CheckPrHead(_) => WorkKind::CheckPrHead,
            Self::CheckWorkingTree { .. } => WorkKind::CheckWorkingTree,
            Self::CheckWorkingCopy(_) => WorkKind::CheckWorkingCopy,
            Self::LookUpPullOfBranch { .. } => WorkKind::LookUpPullOfBranch,
            Self::LookUpHeadBranch(_) => WorkKind::LookUpHeadBranch,
            Self::PullPrDescription(_) => WorkKind::PullPrDescription,
            Self::PullRelatedIssues(_) => WorkKind::PullRelatedIssues,
            Self::PullPrOrigin(_) => WorkKind::PullPrOrigin,
            Self::PullIssueDetails(_) => WorkKind::PullIssueDetails,
            Self::SyncViewed(_) => WorkKind::SyncViewed,
            Self::PullThreads { .. } => WorkKind::PullThreads,
            Self::PullPendingReview(_) => WorkKind::PullPendingReview,
            Self::PullReviews(_) => WorkKind::PullReviews,
            Self::PullPrStatus(_) => WorkKind::PullPrStatus,
            Self::SendQueuedComment(_) => WorkKind::SendQueuedComment,
            Self::ChangeReviewingMark { .. } => WorkKind::ChangeReviewingMark,
            Self::ChangePrReadiness { .. } => WorkKind::ChangePrReadiness,
            Self::ChangeMerging { .. } => WorkKind::ChangeMerging,
            Self::TakeSeenMarksOff(_) => WorkKind::TakeSeenMarksOff,
            Self::DeliverHandOff(_) | Self::DeliverFixRequests(_) => WorkKind::DeliverHandOff,
            Self::CheckAgentLink(_) => WorkKind::CheckAgentLink,
        }
    }
}

impl Work {
    pub fn kind(&self) -> WorkKind {
        self.job.kind()
    }

    pub fn run(self, ports: &WorkPorts<'_>) -> WorkOutcome {
        let kind = self.kind();
        let host = ports.host;
        let done = match self.job {
            Job::CheckPrHead(pr) => Done::PrHead(host.pull_request_tips(&pr)),
            Job::CheckWorkingTree {
                change_set,
                snapshot,
            } => Done::WorkingTree(
                ports
                    .source
                    .change_set_on_disk(&change_set, &snapshot)
                    .map_err(|error| format!("{error:#}")),
            ),
            Job::CheckWorkingCopy(head) => {
                Done::WorkingCopy(ports.source.working_tree_is_at(&head))
            }
            Job::LookUpPullOfBranch { repo, branch } => {
                Done::PullOfBranch(host.open_pull_of_branch(&repo, &branch))
            }
            Job::LookUpHeadBranch(pr) => Done::HeadBranch(host.pull_request_head_branch(&pr)),
            Job::PullPrDescription(pr) => Done::PrDescription(host.pull_request_description(&pr)),
            Job::PullRelatedIssues(pr) => Done::RelatedIssues(host.pull_request_issues(&pr)),
            Job::PullPrOrigin(pr) => Done::PrOrigin(host.pull_request_origin(&pr)),
            Job::PullIssueDetails(issues) => Done::IssueDetails(match host.issue_tracker() {
                Some(tracker) => tracker.issue_details(&issues),
                None => Err(HostError::NotOffered(Capability::IssueTracker)),
            }),
            Job::SyncViewed(request) => Done::SyncViewed(request.run(host)),
            // Outdated threads are followed to their lines in the same trip, so they never jump once shown.
            Job::PullThreads { home, head } => Done::Threads(match &home {
                ThreadHome::Host(pr) => host.review_threads(pr).map(|threads| {
                    let followed = follow_outdated(&threads, ports.source, &head);
                    (threads, followed)
                }),
                ThreadHome::StateFile(store) => store
                    .threads_and_followed_at(ports.source, &head)
                    .map_err(HostError::Other),
            }),
            Job::PullPendingReview(pr) => Done::PendingReview {
                exists: host.has_pending_review(&pr),
                authored: host.viewer_authored(&pr),
            },
            Job::PullReviews(pr) => Done::Reviews(host.reviews(&pr)),
            Job::PullPrStatus(pr) => {
                let status = host.pull_request_status(&pr);
                let merging = match (&status, host.merging()) {
                    (Ok(PrStatus::Open(_)), Some(merging)) => {
                        Some(merging.pull_request_merging(&pr))
                    }
                    _ => None,
                };
                Done::PrStatus { status, merging }
            }
            Job::SendQueuedComment(request) => Done::QueuedComment(request.run(host)),
            Job::ChangeReviewingMark { pr, change } => Done::ReviewingMark {
                change,
                result: match host.pull_request_reactions() {
                    Some(reactions) => {
                        reactions.set_pull_request_reaction(&pr, REVIEWING_MARK, change)
                    }
                    None => Err(HostError::NotOffered(Capability::PullRequestReactions)),
                },
            },
            Job::ChangePrReadiness { pr, readiness } => Done::PrReadiness {
                readiness,
                result: host.set_pull_request_readiness(&pr, readiness),
            },
            Job::ChangeMerging { pr, change } => Done::Merging {
                change,
                result: match host.merging() {
                    Some(merging) => merging.change_merging(&pr, change),
                    None => Err(HostError::NotOffered(Capability::Merging)),
                },
            },
            Job::TakeSeenMarksOff(requests) => Done::SeenMarksOff(
                requests
                    .into_iter()
                    .map(|request| request.run(host))
                    .collect(),
            ),
            Job::DeliverHandOff(request) => Done::Delivery(request.run(ports.agents, host)),
            Job::DeliverFixRequests(request) => Done::FixDelivery(request.run(ports.agents)),
            Job::CheckAgentLink(pr) => Done::AgentLink(ports.agents.lookup(&pr)),
        };
        WorkOutcome {
            kind,
            ticket: self.ticket,
            done,
        }
    }
}

/// What a `Work` brought back, for `Review::land`.
pub struct WorkOutcome {
    pub(super) kind: WorkKind,
    pub(super) ticket: Ticket,
    pub(super) done: Done,
}

pub(super) enum Done {
    PrHead(Result<ChangeTips, HostError>),
    WorkingTree(Result<OnDisk, String>),
    WorkingCopy(bool),
    PullOfBranch(Result<Option<PullOfBranch>, HostError>),
    HeadBranch(Result<Option<BranchName>, HostError>),
    PrDescription(Result<String, HostError>),
    RelatedIssues(Result<PullRequestIssues, HostError>),
    PrOrigin(Result<PullRequestOrigin, HostError>),
    IssueDetails(Result<Vec<(IssueUrl, IssueDetails)>, HostError>),
    SyncViewed(SyncOutcome),
    Threads(Result<(Vec<ReviewThread>, BTreeMap<ThreadId, FollowedLines>), HostError>),
    PendingReview {
        exists: Result<bool, HostError>,
        authored: Result<bool, HostError>,
    },
    Reviews(Result<PullRequestReviews, HostError>),
    /// `merging` is read only for an open PR on a host that merges from the app.
    PrStatus {
        status: Result<PrStatus, HostError>,
        merging: Option<Result<Merging, HostError>>,
    },
    QueuedComment(CommentOutcome),
    ReviewingMark {
        change: ReactionChange,
        result: Result<(), HostError>,
    },
    PrReadiness {
        readiness: Readiness,
        result: Result<(), HostError>,
    },
    Merging {
        change: MergeChange,
        result: Result<(), HostError>,
    },
    SeenMarksOff(Vec<SeenMarkOffOutcome>),
    Delivery(DeliveryOutcome),
    FixDelivery(FixDeliveryOutcome),
    AgentLink(TourAgentState),
}

/// What the app does once an outcome landed, besides showing the review as it is now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AfterLanding {
    Nothing,
    /// A tour agent was started for a hand-off, so there is a new terminal to follow.
    FollowNewTourAgent,
    /// A look at herdr after the link was off: what the sidebar says now, and when it is not off
    /// any more, the tour agent to follow.
    TourAgentIs(TourAgentState),
}

impl Affected {
    /// The work that reads it again.
    pub(super) fn retried_by(self) -> WorkKind {
        match self {
            Self::NewCommits => WorkKind::CheckPrHead,
            Self::Description => WorkKind::PullPrDescription,
            Self::Comments => WorkKind::PullThreads,
            Self::ViewedMarks => WorkKind::SyncViewed,
            Self::PendingReview | Self::Authorship => WorkKind::PullPendingReview,
            Self::LastReview => WorkKind::PullReviews,
            Self::PrStatus => WorkKind::PullPrStatus,
        }
    }
}
