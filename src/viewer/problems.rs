use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;
use std::time::Duration;

use crate::update::InTheWay;

use super::github_outage::{Affected, Outage, OutageCause, waiting};
use super::saved_state::RepinRunning;
use super::toasts::ToastTone;

/// Something that went wrong which the reviewer must know about, shown until it is put right.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Problem {
    /// Ticks and typed text are only in memory until a save goes through.
    NotSaved(String),
    /// A tick or typed text made while the re-pin ran; the re-pinned review will not have it.
    ChangeNotKeptWhileRepinning,
    /// The threads shown are as GitHub had them at the last read that went through.
    ThreadsNotRead(String),
    /// GitHub does not answer: one problem for all the reads that failed because of it.
    GitHubNotAnswering(Outage),
    /// A read from GitHub failed, so what the screen shows for it is missing or as of an earlier read.
    NotRead(Read, String),
    /// A code link was clicked and its IDE command could not be run, so nothing opened.
    IdeNotStarted(String),
    /// The local clone could not give the commits, so code opens on GitHub and not in the IDE.
    CodeOnGitHub(String),
    /// The saved review could not be read, so this one starts empty; the old file is kept at the path.
    SavedReviewNotRead(PathBuf),
    /// Ticks wait in the queue to reach GitHub's viewed marks, and GitHub may show them as unviewed.
    ViewedNotSynced(String),
    /// The AI Refresh ended and the steps marked as old are still as they were.
    TourNotRefreshed(String),
    /// `open` named the PR shown, and its tour file could not be read again.
    TourNotReloaded(String),
    /// A comment sent with Ask agent was posted, but GitHub showed no thread for it, so the agent never gets it.
    HandOffNotSent { comment: String, reason: String },
    /// Another PR was asked for and could not be opened, so this one stays on screen.
    SwitchFailed(String),
    /// Show agent was clicked and the terminal did not come up.
    ShowAgentFailed(String),
    /// GitHub refused to put the 👀 on the PR or take it off, so its author may read the wrong thing.
    ReviewingMarkNotChanged(String),
    /// Another `guided-review` runs in place of the release install, so updates never reach the reviewer.
    ReleaseInstallInTheWay(InTheWay),
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
    GitHubNotAnswering,
    /// Cleared by a good read of the same thing from GitHub.
    NotRead(Read),
    /// Cleared by the next IDE command that runs.
    IdeNotStarted,
    /// Raised when the review opens; a review opened on the clone has none.
    CodeOnGitHub,
    /// Cleared by a sync of viewed marks that goes through.
    ViewedNotSynced,
    /// Stays: a Refresh that goes through opens a new review.
    TourNotRefreshed,
    /// Stays: a reload that goes through opens a new review.
    TourNotReloaded,
    /// Raised when the review opens; the old file is kept, so nothing clears it.
    SavedReviewNotRead,
    /// Replaced by the next hand-off, which clears it when that one is sent.
    HandOffNotSent,
    /// Stays: a switch that goes through opens another review.
    SwitchFailed,
    /// Cleared by the next Show agent that brings the terminal up.
    ShowAgentFailed,
    /// Cleared by the next change of the 👀 on the PR that goes through.
    ReviewingMarkNotChanged,
    /// Raised when the review opens; a review opened once the binary is removed has none.
    ReleaseInstallInTheWay,
}

impl Problem {
    pub fn kind(&self) -> ProblemKind {
        match self {
            Self::NotSaved(_) => ProblemKind::NotSaved,
            Self::ChangeNotKeptWhileRepinning => ProblemKind::ChangeNotKeptWhileRepinning,
            Self::ThreadsNotRead(_) => ProblemKind::ThreadsNotRead,
            Self::GitHubNotAnswering(_) => ProblemKind::GitHubNotAnswering,
            Self::NotRead(read, _) => ProblemKind::NotRead(*read),
            Self::IdeNotStarted(_) => ProblemKind::IdeNotStarted,
            Self::CodeOnGitHub(_) => ProblemKind::CodeOnGitHub,
            Self::SavedReviewNotRead(_) => ProblemKind::SavedReviewNotRead,
            Self::ViewedNotSynced(_) => ProblemKind::ViewedNotSynced,
            Self::TourNotRefreshed(_) => ProblemKind::TourNotRefreshed,
            Self::TourNotReloaded(_) => ProblemKind::TourNotReloaded,
            Self::HandOffNotSent { .. } => ProblemKind::HandOffNotSent,
            Self::SwitchFailed(_) => ProblemKind::SwitchFailed,
            Self::ShowAgentFailed(_) => ProblemKind::ShowAgentFailed,
            Self::ReviewingMarkNotChanged(_) => ProblemKind::ReviewingMarkNotChanged,
            Self::ReleaseInstallInTheWay(_) => ProblemKind::ReleaseInstallInTheWay,
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

    /// A read failed because GitHub does not answer. Returns how long to wait before it is tried again.
    pub fn github_not_answering(&mut self, cause: OutageCause, affected: Affected) -> Duration {
        let mut outage = match self.raised.remove(&ProblemKind::GitHubNotAnswering) {
            Some(Problem::GitHubNotAnswering(outage)) => outage,
            _ => Outage::new(cause),
        };
        let retry_in = outage.failed(cause, affected);
        self.raise(Problem::GitHubNotAnswering(outage));
        retry_in
    }

    /// GitHub answered a read, with the data or with an error of its own: it is answering.
    pub fn github_answered(&mut self, affected: Affected) {
        let Some(Problem::GitHubNotAnswering(mut outage)) =
            self.raised.remove(&ProblemKind::GitHubNotAnswering)
        else {
            return;
        };
        if !outage.answered(affected) {
            self.raise(Problem::GitHubNotAnswering(outage));
        }
    }

    /// In the order of the kinds.
    pub fn list(&self) -> impl Iterator<Item = &Problem> {
        self.raised.values()
    }
}

/// What the app reads from GitHub that has no problem of its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Read {
    PrHead,
    PrDescription,
    PendingReview,
    Authorship,
    LastReview,
}

impl Problem {
    /// A line under the problem, for what the headline leaves out.
    pub fn detail(&self) -> Option<String> {
        let Self::GitHubNotAnswering(outage) = self else {
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
            Self::GitHubNotAnswering(outage) => Announcement {
                tone: ToastTone::Neutral,
                text: format!("GitHub is not answering ({})", outage.cause()),
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
            (Self::GitHubNotAnswering(_), Self::GitHubNotAnswering(_)) => true,
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
    let is_outage = |problem: &Problem| matches!(problem, Problem::GitHubNotAnswering(_));
    let new = after
        .iter()
        .filter(|problem| !before.iter().any(|earlier| problem.is_told_by(earlier)))
        .map(Problem::announcement);
    let back =
        (before.iter().any(is_outage) && !after.iter().any(is_outage)).then(|| Announcement {
            tone: ToastTone::Success,
            text: "GitHub is back".to_owned(),
        });
    new.chain(back).collect()
}

impl fmt::Display for Problem {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::GitHubNotAnswering(outage) => write!(
                formatter,
                "GitHub is not answering ({}). Some things may be out of date; trying again in {}.",
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
                "Could not look for a pending review on GitHub, so Comment now may be wrong: {reason}"
            ),
            Self::NotRead(Read::Authorship, reason) => write!(
                formatter,
                "Could not tell who wrote the PR, so Approve and Request changes may be wrong: {reason}"
            ),
            Self::NotRead(Read::LastReview, reason) => write!(
                formatter,
                "Your last review could not be read, so Finish review may not show it: {reason}"
            ),
            Self::IdeNotStarted(reason) => write!(
                formatter,
                "Could not start the IDE command, so code links open nothing: {reason}"
            ),
            Self::CodeOnGitHub(reason) => write!(
                formatter,
                "Code opens on GitHub, not in the IDE, as the local clone could not give the commits: {reason}"
            ),
            Self::SavedReviewNotRead(backup) => write!(
                formatter,
                "The saved review could not be read, so this review starts empty. The old file is kept as {}",
                backup.display()
            ),
            Self::ViewedNotSynced(reason) => write!(
                formatter,
                "Viewed marks are not synced with GitHub, they are retried on the next sync: {reason}"
            ),
            Self::TourNotRefreshed(reason) => {
                write!(formatter, "The tour was not refreshed: {reason}")
            }
            Self::TourNotReloaded(reason) => write!(
                formatter,
                "The tour could not be read again, so it shows as it was: {reason}"
            ),
            Self::HandOffNotSent { comment, reason } => write!(
                formatter,
                "The agent will not get your comment \"{comment}\": {reason}"
            ),
            Self::SwitchFailed(reason) => write!(formatter, "Not switched: {reason}"),
            Self::ShowAgentFailed(reason) => {
                write!(formatter, "Could not show the agent: {reason}")
            }
            Self::ReviewingMarkNotChanged(reason) => write!(
                formatter,
                "The 👀 on the PR may tell its author the wrong thing; trying again when the window comes to the front: {reason}"
            ),
            Self::ReleaseInstallInTheWay(InTheWay::Runs {
                binary,
                release_install,
            }) => write!(
                formatter,
                "Updates do not reach this app: it runs {binary}, not the release install \
                 {release_install}. Remove {binary} and start guided-review again.",
                binary = binary.display(),
                release_install = release_install.display(),
            ),
            Self::ReleaseInstallInTheWay(InTheWay::FirstOnPath {
                binary,
                release_install,
            }) => write!(
                formatter,
                "Updates do not reach guided-review in a terminal or the tour skill: {binary} comes \
                 before the release install {release_install} on PATH. Remove {binary}.",
                binary = binary.display(),
                release_install = release_install.display(),
            ),
        }
    }
}
