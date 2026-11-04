use std::fmt;

use super::review::RepinRunning;

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
}

/// What the app reads from GitHub that has no problem of its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Read {
    PrHead,
    PrDescription,
    PendingReview,
    Authorship,
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
        }
    }
}
