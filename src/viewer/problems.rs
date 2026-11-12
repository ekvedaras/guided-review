use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;

use crate::update::InTheWay;

use super::saved_state::RepinRunning;

/// Something that went wrong which the reviewer must know about, shown until it is put right.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Problem {
    /// Ticks and typed text are only in memory until a save goes through.
    NotSaved(String),
    /// A tick or typed text made while the re-pin ran; the re-pinned review will not have it.
    ChangeNotKeptWhileRepinning,
    /// The threads shown are as GitHub had them at the last read that went through.
    ThreadsNotRead(String),
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
    /// Raised when the review opens; a review opened once the binary is removed has none.
    ReleaseInstallInTheWay,
}

impl Problem {
    pub fn kind(&self) -> ProblemKind {
        match self {
            Self::NotSaved(_) => ProblemKind::NotSaved,
            Self::ChangeNotKeptWhileRepinning => ProblemKind::ChangeNotKeptWhileRepinning,
            Self::ThreadsNotRead(_) => ProblemKind::ThreadsNotRead,
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

impl fmt::Display for Problem {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::NotSaved(reason) => write!(formatter, "Not saved to disk: {reason}"),
            Self::ChangeNotKeptWhileRepinning => write!(formatter, "Not kept: {RepinRunning}"),
            Self::ThreadsNotRead(reason) => write!(
                formatter,
                "Comments may be out of date, could not read them from GitHub: {reason}"
            ),
            Self::NotRead(Read::PrHead, reason) => write!(
                formatter,
                "New commits on the PR are not checked, could not read its head from GitHub: {reason}"
            ),
            Self::NotRead(Read::PrDescription, reason) => write!(
                formatter,
                "The PR description may be out of date, could not read it from GitHub: {reason}"
            ),
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
                "Could not read your last review from GitHub, so Finish review may not show it: {reason}"
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
