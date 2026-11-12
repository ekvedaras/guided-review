//! The work a review wants done off the UI thread: which of it is due, which runs, and what is
//! dropped. The review decides what each outcome makes due next; the app only runs each `Work`
//! with the ports and lands its outcome, and tests drive the same loop in line.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use crate::code_source::CodeSource;
use crate::pull_request::{PullRequestTips, PullRequestUrl, Sha};

use super::comments::{CommentOutcome, CommentRequest};
use super::finish_review::SubmittedReview;
use super::hand_offs::{DeliveryOutcome, DeliveryRequest, SeenMarkOff, SeenMarkOffOutcome};
use super::outdated_lines::{FollowedLines, follow_outdated};
use super::ports::{GitHubError, GitHubPort};
use super::threads::{ReviewThread, ThreadId};
use super::tour_agent::TourAgents;
use super::viewed_sync::{SyncOutcome, SyncRequest};

/// A tick's sync waits this long for the next tick, so a burst of ticks is one round trip.
pub const PUSH_DEBOUNCE: Duration = Duration::from_secs(1);
/// How long to wait before handing off again to an agent whose screen had not settled since it started.
pub const AGENT_START_RETRY: Duration = Duration::from_secs(3);

/// The kinds of work, in the order due work of several kinds starts in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum WorkKind {
    CheckPrHead,
    CheckWorkingCopy,
    PullPrDescription,
    SyncViewed,
    PullThreads,
    PullPendingReview,
    PullLastReview,
    SendQueuedComment,
    TakeSeenMarksOff,
    DeliverHandOff,
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
    /// What a window coming to the front reads and sends again.
    pub(super) const PULLS_FROM_GITHUB: [Self; 8] = [
        Self::CheckPrHead,
        Self::CheckWorkingCopy,
        Self::PullPrDescription,
        Self::SyncViewed,
        Self::PullThreads,
        Self::PullPendingReview,
        Self::PullLastReview,
        Self::SendQueuedComment,
    ];

    fn while_running(self) -> WhileRunning {
        match self {
            Self::SyncViewed => WhileRunning::RunsAgainAfter,
            Self::SendQueuedComment | Self::TakeSeenMarksOff | Self::DeliverHandOff => {
                WhileRunning::IsDropped
            }
            Self::CheckPrHead
            | Self::CheckWorkingCopy
            | Self::PullPrDescription
            | Self::PullThreads
            | Self::PullPendingReview
            | Self::PullLastReview => WhileRunning::Replaces,
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
    pub github: &'a dyn GitHubPort,
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
    CheckWorkingCopy(Sha),
    PullPrDescription(PullRequestUrl),
    SyncViewed(SyncRequest),
    PullThreads { pr: PullRequestUrl, head: Sha },
    PullPendingReview(PullRequestUrl),
    PullLastReview(PullRequestUrl),
    SendQueuedComment(CommentRequest),
    TakeSeenMarksOff(Vec<SeenMarkOff>),
    DeliverHandOff(DeliveryRequest),
}

impl Job {
    fn kind(&self) -> WorkKind {
        match self {
            Self::CheckPrHead(_) => WorkKind::CheckPrHead,
            Self::CheckWorkingCopy(_) => WorkKind::CheckWorkingCopy,
            Self::PullPrDescription(_) => WorkKind::PullPrDescription,
            Self::SyncViewed(_) => WorkKind::SyncViewed,
            Self::PullThreads { .. } => WorkKind::PullThreads,
            Self::PullPendingReview(_) => WorkKind::PullPendingReview,
            Self::PullLastReview(_) => WorkKind::PullLastReview,
            Self::SendQueuedComment(_) => WorkKind::SendQueuedComment,
            Self::TakeSeenMarksOff(_) => WorkKind::TakeSeenMarksOff,
            Self::DeliverHandOff(_) => WorkKind::DeliverHandOff,
        }
    }
}

impl Work {
    pub fn kind(&self) -> WorkKind {
        self.job.kind()
    }

    pub fn run(self, ports: &WorkPorts<'_>) -> WorkOutcome {
        let kind = self.kind();
        let github = ports.github;
        let done = match self.job {
            Job::CheckPrHead(pr) => Done::PrHead(github.pull_request_tips(&pr)),
            Job::CheckWorkingCopy(head) => {
                Done::WorkingCopy(ports.source.working_tree_is_at(&head))
            }
            Job::PullPrDescription(pr) => Done::PrDescription(github.pull_request_description(&pr)),
            Job::SyncViewed(request) => Done::SyncViewed(request.run(github)),
            // Outdated threads are followed to their lines in the same trip, so they never jump once shown.
            Job::PullThreads { pr, head } => {
                Done::Threads(github.review_threads(&pr).map(|threads| {
                    let followed = follow_outdated(&threads, ports.source, &head);
                    (threads, followed)
                }))
            }
            Job::PullPendingReview(pr) => Done::PendingReview {
                exists: github.has_pending_review(&pr),
                authored: github.viewer_authored(&pr),
            },
            Job::PullLastReview(pr) => Done::LastReview(github.viewer_latest_review(&pr)),
            Job::SendQueuedComment(request) => Done::QueuedComment(request.run(github)),
            Job::TakeSeenMarksOff(requests) => Done::SeenMarksOff(
                requests
                    .into_iter()
                    .map(|request| request.run(github))
                    .collect(),
            ),
            Job::DeliverHandOff(request) => Done::Delivery(request.run(ports.agents, github)),
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
    PrHead(Result<PullRequestTips, GitHubError>),
    WorkingCopy(bool),
    PrDescription(Result<String, GitHubError>),
    SyncViewed(SyncOutcome),
    Threads(Result<(Vec<ReviewThread>, BTreeMap<ThreadId, FollowedLines>), GitHubError>),
    PendingReview {
        exists: Result<bool, GitHubError>,
        authored: Result<bool, GitHubError>,
    },
    LastReview(Result<Option<SubmittedReview>, GitHubError>),
    QueuedComment(CommentOutcome),
    SeenMarksOff(Vec<SeenMarkOffOutcome>),
    Delivery(DeliveryOutcome),
}

/// What the app does once an outcome landed, besides showing the review as it is now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AfterLanding {
    Nothing,
    /// A tour agent was started for a hand-off, so there is a new terminal to follow.
    FollowNewTourAgent,
}
