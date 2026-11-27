use super::host_outage::Read;
use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;
use std::time::Duration;

use crate::app_folders::NotRead;
use crate::local_reviews_on_disk::UnreadableLocalTour;
use crate::pull_request::HostSite;
use crate::update::InTheWay;

use super::host_outage::{Affected, Outage, OutageCause, waiting};
use super::merging::MergeChange;
use super::pr_status::Readiness;
use super::saved_state::RepinRunning;
use super::toasts::ToastTone;

/// Something that went wrong which the reviewer must know about, shown until it is put right.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Problem {
    /// Ticks and typed text are only in memory until a save goes through.
    NotSaved(String),
    /// A tick or typed text made while the re-pin ran; the re-pinned review will not have it.
    ChangeNotKeptWhileRepinning,
    /// The threads shown are as the host had them at the last read that went through.
    ThreadsNotRead(String),
    /// The host does not answer: one problem for all the reads that failed because of it.
    HostNotAnswering(Outage),
    /// A read from the host failed, so what the screen shows for it is missing or as of an earlier read.
    NotRead(Read, String),
    /// A code link was clicked and its IDE command could not be run, so nothing opened.
    IdeNotStarted(String),
    /// The local clone could not give the commits, so code opens on the PR's host and not in the IDE.
    CodeOnHost(HostSite, String),
    /// The saved review could not be read, so this one starts empty; the old file is kept at the path.
    SavedReviewNotRead(PathBuf),
    /// Parts of the saved review a newer build wrote, which this build leaves in the file as they are.
    SavedReviewPartsNotRead(String),
    /// Ticks wait in the queue to reach the host's viewed marks, and the host may show them as unviewed.
    ViewedNotSynced(String),
    /// The AI Refresh ended and the steps marked as old are still as they were.
    TourNotRefreshed(String),
    /// `open` named the PR shown, and its tour file could not be read again.
    TourNotReloaded(String),
    /// Drop review of a local review failed part way; the review stays on screen.
    ReviewNotDropped(String),
    /// git could not say how a local review's folder stands against its snapshot, so the sidebar
    /// says what the last look that went through found.
    FolderNotLookedAt(String),
    /// Not now was clicked on a move offer and the choice could not be written, so the offer comes back.
    MoveNotDeclined(String),
    /// A comment sent with Comment and ask agent was posted, but the host showed no thread for it, so the agent never gets it.
    HandOffNotSent { comment: String, reason: String },
    /// Another PR was asked for and could not be opened, so this one stays on screen.
    SwitchFailed(String),
    /// Show agent was clicked and the terminal did not come up.
    ShowAgentFailed(String),
    /// An agent was picked and the terminal could not be made the change set's agent.
    UseAgentFailed(String),
    /// The host refused to put the 👀 on the PR or take it off, so its author may read the wrong thing.
    ReviewingMarkNotChanged(String),
    /// The host did not take the change of the PR between draft and ready.
    ReadinessNotChanged(Readiness, String),
    /// The host did not turn auto-merge on or off, or did not merge the PR.
    MergingNotChanged(MergeChange, String),
    /// Another `guided-review` runs in place of the release install, so updates never reach the reviewer.
    ReleaseInstallInTheWay(InTheWay),
    /// Files the app keeps that lie in the app folders it does not use: what a move from
    /// `~/.guided-review` left behind or could not put back, or what an older binary wrote there since.
    FilesNotRead(Vec<NotRead>),
    /// Local tours that could not be read: the app keeps every snapshot ref of their clones, and the
    /// reviews they hold do not open.
    LocalToursNotRead(Vec<UnreadableLocalTour>),
}

/// What a problem is about. A problem of a kind stands in for the one of that kind before it, and
/// its kind names what clears it. The order of the kinds is the order the sidebar lists them in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ProblemKind {
    /// Kept by the saved state, not raised: gone when a save goes through.
    NotSaved,
    /// Kept by the saved state, not raised: gone when the re-pin ends.
    ChangeNotKeptWhileRepinning,
    /// Cleared by a read of the threads that goes through.
    ThreadsNotRead,
    /// Cleared when each read it keeps from going through goes through.
    HostNotAnswering,
    /// Cleared by a good read of the same thing from the host.
    NotRead(Read),
    /// Cleared by the next IDE command that runs.
    IdeNotStarted,
    /// Raised when the review opens; a review opened on the clone has none.
    CodeOnHost,
    /// Cleared by a sync of viewed marks that goes through.
    ViewedNotSynced,
    /// Stays: a Refresh that goes through opens a new review.
    TourNotRefreshed,
    /// Stays: a reload that goes through opens a new review.
    TourNotReloaded,
    /// Stays: a drop that goes through closes the review.
    ReviewNotDropped,
    /// Cleared by a look at the folder that goes through.
    FolderNotLookedAt,
    /// Raised when the review opens; the old file is kept, so nothing clears it.
    SavedReviewNotRead,
    /// Raised when the review opens; the parts stay in the file, so nothing clears it.
    SavedReviewPartsNotRead,
    /// Cleared by the next Not now or Keep local review that is written.
    MoveNotDeclined,
    /// Replaced by the next hand-off, which clears it when that one is sent.
    HandOffNotSent,
    /// Stays: a switch that goes through opens another review.
    SwitchFailed,
    /// Cleared by the next Show agent that brings the terminal up.
    ShowAgentFailed,
    /// Cleared by the next pick of an agent that goes through.
    UseAgentFailed,
    /// Cleared by the next change of the 👀 on the PR that goes through.
    ReviewingMarkNotChanged,
    /// Cleared by the next change of the PR between draft and ready that goes through.
    ReadinessNotChanged,
    /// Cleared by the next auto-merge change or merge that goes through.
    MergingNotChanged,
    /// Raised when the review opens; a review opened once the binary is removed has none.
    ReleaseInstallInTheWay,
    /// Raised when the review opens; a review opened once the files are gone has none.
    FilesNotRead,
    /// Raised when the review opens; a review opened once the tours are readable or gone has none.
    LocalToursNotRead,
}

impl Problem {
    pub fn kind(&self) -> ProblemKind {
        match self {
            Self::NotSaved(_) => ProblemKind::NotSaved,
            Self::ChangeNotKeptWhileRepinning => ProblemKind::ChangeNotKeptWhileRepinning,
            Self::ThreadsNotRead(_) => ProblemKind::ThreadsNotRead,
            Self::HostNotAnswering(_) => ProblemKind::HostNotAnswering,
            Self::NotRead(read, _) => ProblemKind::NotRead(*read),
            Self::IdeNotStarted(_) => ProblemKind::IdeNotStarted,
            Self::CodeOnHost(..) => ProblemKind::CodeOnHost,
            Self::SavedReviewNotRead(_) => ProblemKind::SavedReviewNotRead,
            Self::SavedReviewPartsNotRead(_) => ProblemKind::SavedReviewPartsNotRead,
            Self::ViewedNotSynced(_) => ProblemKind::ViewedNotSynced,
            Self::TourNotRefreshed(_) => ProblemKind::TourNotRefreshed,
            Self::TourNotReloaded(_) => ProblemKind::TourNotReloaded,
            Self::ReviewNotDropped(_) => ProblemKind::ReviewNotDropped,
            Self::FolderNotLookedAt(_) => ProblemKind::FolderNotLookedAt,
            Self::MoveNotDeclined(_) => ProblemKind::MoveNotDeclined,
            Self::HandOffNotSent { .. } => ProblemKind::HandOffNotSent,
            Self::SwitchFailed(_) => ProblemKind::SwitchFailed,
            Self::ShowAgentFailed(_) => ProblemKind::ShowAgentFailed,
            Self::UseAgentFailed(_) => ProblemKind::UseAgentFailed,
            Self::ReviewingMarkNotChanged(_) => ProblemKind::ReviewingMarkNotChanged,
            Self::ReadinessNotChanged(..) => ProblemKind::ReadinessNotChanged,
            Self::MergingNotChanged(..) => ProblemKind::MergingNotChanged,
            Self::ReleaseInstallInTheWay(_) => ProblemKind::ReleaseInstallInTheWay,
            Self::FilesNotRead(_) => ProblemKind::FilesNotRead,
            Self::LocalToursNotRead(_) => ProblemKind::LocalToursNotRead,
        }
    }
}

/// The problems a review holds, one per kind. Those of the saved state are not held here.
#[derive(Debug, Default)]
pub struct Problems {
    raised: BTreeMap<ProblemKind, Problem>,
}

impl Problems {
    pub fn raise(&mut self, problem: Problem) {
        self.raised.insert(problem.kind(), problem);
    }

    pub fn clear(&mut self, kind: ProblemKind) {
        self.raised.remove(&kind);
    }

    /// A read failed because the host does not answer. Returns how long to wait before it is tried again.
    pub fn host_not_answering(
        &mut self,
        host: HostSite,
        cause: OutageCause,
        affected: Affected,
    ) -> Duration {
        let mut outage = match self.raised.remove(&ProblemKind::HostNotAnswering) {
            Some(Problem::HostNotAnswering(outage)) => outage,
            _ => Outage::new(host, cause),
        };
        let retry_in = outage.failed(cause, affected);
        self.raise(Problem::HostNotAnswering(outage));
        retry_in
    }

    /// The host answered a read, with the data or with an error of its own: it is answering.
    pub fn host_answered(&mut self, affected: Affected) {
        let Some(Problem::HostNotAnswering(mut outage)) =
            self.raised.remove(&ProblemKind::HostNotAnswering)
        else {
            return;
        };
        if !outage.answered(affected) {
            self.raise(Problem::HostNotAnswering(outage));
        }
    }

    /// In the order of the kinds.
    pub fn list(&self) -> impl Iterator<Item = &Problem> {
        self.raised.values()
    }
}

impl Problem {
    /// A line under the problem, for what the headline leaves out.
    pub fn detail(&self) -> Option<String> {
        let Self::HostNotAnswering(outage) = self else {
            return None;
        };
        let affected: Vec<String> = outage
            .affected()
            .map(|affected| affected.to_string())
            .collect();
        Some(format!("Not checked: {}.", affected.join(", ")))
    }

    /// What the reviewer is told when this problem is new. An outage is not the reviewer's failure
    /// and the sidebar keeps it, so its note leaves by itself; other problems stay until dismissed.
    pub fn announcement(&self) -> Announcement {
        match self {
            Self::HostNotAnswering(outage) => Announcement {
                tone: ToastTone::Neutral,
                text: format!(
                    "{} is not answering ({})",
                    outage.host().name(),
                    outage.cause()
                ),
            },
            other => Announcement {
                tone: ToastTone::Danger,
                text: other.to_string(),
            },
        }
    }

    /// An outage is one story whatever its cause, retry time and reads become; any other problem is
    /// told again when its words change.
    fn is_told_by(&self, earlier: &Self) -> bool {
        match (self, earlier) {
            (Self::HostNotAnswering(_), Self::HostNotAnswering(_)) => true,
            _ => self == earlier,
        }
    }
}

pub struct Announcement {
    pub tone: ToastTone,
    pub text: String,
}

/// The notes to show now that the problems went from `before` to `after`.
pub fn announcements(before: &[Problem], after: &[Problem]) -> Vec<Announcement> {
    let outage_of = |problems: &[Problem]| {
        problems.iter().find_map(|problem| match problem {
            Problem::HostNotAnswering(outage) => Some(outage.host()),
            _ => None,
        })
    };
    let new = after
        .iter()
        .filter(|problem| !before.iter().any(|earlier| problem.is_told_by(earlier)))
        .map(Problem::announcement);
    let back = outage_of(before)
        .filter(|_| outage_of(after).is_none())
        .map(|host| Announcement {
            tone: ToastTone::Success,
            text: format!("{} is back", host.name()),
        });
    new.chain(back).collect()
}

impl fmt::Display for Problem {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::HostNotAnswering(outage) => write!(
                formatter,
                "{} is not answering ({}). Some things may be out of date; trying again in {}.",
                outage.host().name(),
                outage.cause(),
                waiting(outage.retry_in())
            ),
            Self::NotSaved(reason) => write!(formatter, "Not saved to disk: {reason}"),
            Self::ChangeNotKeptWhileRepinning => write!(formatter, "Not kept: {RepinRunning}"),
            Self::ThreadsNotRead(reason) => {
                write!(formatter, "Comments may be out of date: {reason}")
            }
            Self::NotRead(Read::PrHead, reason) => {
                write!(formatter, "New commits on the PR are not checked: {reason}")
            }
            Self::NotRead(Read::PrDescription, reason) => {
                write!(formatter, "The PR description may be out of date: {reason}")
            }
            Self::NotRead(Read::PendingReview, reason) => write!(
                formatter,
                "Could not look for a pending review, so Comment now may be wrong: {reason}"
            ),
            Self::NotRead(Read::Authorship, reason) => write!(
                formatter,
                "Could not tell who wrote the PR, so Approve and Request changes may be wrong: {reason}"
            ),
            Self::NotRead(Read::LastReview, reason) => write!(
                formatter,
                "Your last review could not be read, so Finish review may not show it: {reason}"
            ),
            Self::NotRead(Read::PrStatus, reason) => write!(
                formatter,
                "Whether the PR is a draft and its checks may be out of date: {reason}"
            ),
            Self::IdeNotStarted(reason) => write!(
                formatter,
                "Could not start the IDE command, so code links open nothing: {reason}"
            ),
            Self::CodeOnHost(host, reason) => write!(
                formatter,
                "Code opens on {}, not in the IDE, as the local clone could not give the commits: {reason}",
                host.name()
            ),
            Self::SavedReviewPartsNotRead(parts) => write!(
                formatter,
                "Parts of the saved review are in a shape this build does not read, likely from a newer build: {parts}. This build does not show them and keeps them in the file as they are"
            ),
            Self::SavedReviewNotRead(backup) => write!(
                formatter,
                "The saved review could not be read, so this review starts empty. The old file is kept as {}",
                backup.display()
            ),
            Self::ViewedNotSynced(reason) => write!(
                formatter,
                "Viewed marks are not synced, they are retried on the next sync: {reason}"
            ),
            Self::TourNotRefreshed(reason) => {
                write!(formatter, "The tour was not refreshed: {reason}")
            }
            Self::TourNotReloaded(reason) => write!(
                formatter,
                "The tour could not be read again, so it shows as it was: {reason}"
            ),
            Self::ReviewNotDropped(reason) => {
                write!(formatter, "The review was not dropped: {reason}")
            }
            Self::FolderNotLookedAt(reason) => write!(
                formatter,
                "The folder could not be looked at, so the sidebar may not say how it changed: {reason}"
            ),
            Self::MoveNotDeclined(reason) => write!(
                formatter,
                "Your choice not to move was not kept, so the offer will come back: {reason}"
            ),
            Self::HandOffNotSent { comment, reason } => write!(
                formatter,
                "The agent will not get your comment \"{comment}\": {reason}"
            ),
            Self::SwitchFailed(reason) => write!(formatter, "Not switched: {reason}"),
            Self::ShowAgentFailed(reason) => {
                write!(formatter, "Could not show the agent: {reason}")
            }
            Self::UseAgentFailed(reason) => {
                write!(formatter, "Could not use that agent: {reason}")
            }
            Self::ReviewingMarkNotChanged(reason) => write!(
                formatter,
                "The 👀 on the PR may tell its author the wrong thing; trying again when the window comes to the front: {reason}"
            ),
            Self::ReadinessNotChanged(Readiness::Draft, reason) => {
                write!(formatter, "The PR was not converted to a draft: {reason}")
            }
            Self::ReadinessNotChanged(Readiness::Ready, reason) => {
                write!(
                    formatter,
                    "The PR was not marked ready for review: {reason}"
                )
            }
            Self::MergingNotChanged(MergeChange::EnableAutoMerge(_), reason) => {
                write!(formatter, "Auto-merge was not turned on: {reason}")
            }
            Self::MergingNotChanged(MergeChange::DisableAutoMerge, reason) => {
                write!(formatter, "Auto-merge was not turned off: {reason}")
            }
            Self::MergingNotChanged(MergeChange::MergeNow(_), reason) => {
                write!(formatter, "The PR was not merged: {reason}")
            }
            Self::ReleaseInstallInTheWay(InTheWay::FirstOnPath { binary, .. }) => write!(
                formatter,
                "{} comes first on PATH, so the tour skill misses updates",
                binary.display()
            ),
            Self::FilesNotRead(files) => {
                const NAMED: usize = 3;
                write!(
                    formatter,
                    "{} {} not read, as the app reads another file in their place; nothing was merged, overwritten or deleted, so keep what you need by hand: ",
                    files.len(),
                    if files.len() == 1 {
                        "file is"
                    } else {
                        "files are"
                    }
                )?;
                let named: Vec<String> = files
                    .iter()
                    .take(NAMED)
                    .map(|not_read| {
                        format!(
                            "{} (read instead: {})",
                            not_read.file.display(),
                            not_read.read_instead.display()
                        )
                    })
                    .collect();
                write!(formatter, "{}", named.join(", "))?;
                match files.len().saturating_sub(NAMED) {
                    0 => Ok(()),
                    more => write!(formatter, " and {more} more"),
                }
            }
            Self::LocalToursNotRead(tours) => {
                const NAMED: usize = 3;
                if tours.len() == 1 {
                    write!(
                        formatter,
                        "A local review's tour could not be read, so the snapshot refs of its clone are kept until it can: "
                    )?;
                } else {
                    write!(
                        formatter,
                        "{} local tours could not be read, so the snapshot refs of their clones are kept until they can: ",
                        tours.len()
                    )?;
                }
                let named: Vec<String> = tours
                    .iter()
                    .take(NAMED)
                    .map(|tour| format!("{} ({})", tour.path.display(), tour.reason))
                    .collect();
                write!(formatter, "{}", named.join(", "))?;
                match tours.len().saturating_sub(NAMED) {
                    0 => Ok(()),
                    more => write!(formatter, " and {more} more"),
                }
            }
        }
    }
}
