use std::fmt;
use std::path::PathBuf;

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
    /// A comment sent with Ask agent was posted, but GitHub showed no thread for it, so the agent never gets it.
    HandOffNotSent { comment: String, reason: String },
    /// Another PR was asked for and could not be opened, so this one stays on screen.
    SwitchFailed(String),
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
            Self::HandOffNotSent { comment, reason } => write!(
                formatter,
                "The agent will not get your comment \"{comment}\": {reason}"
            ),
            Self::SwitchFailed(reason) => write!(formatter, "Not switched: {reason}"),
        }
    }
}
