use std::fmt;

use super::review::RepinRunning;

/// Something that went wrong which the reviewer must know about, shown until it is put right.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Problem {
    /// Ticks and typed text are only in memory until a save goes through.
    NotSaved(String),
    /// A tick or typed text made while the re-pin ran; the re-pinned review will not have it.
    ChangeNotKeptWhileRepinning,
}

impl fmt::Display for Problem {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::NotSaved(reason) => write!(formatter, "Not saved to disk: {reason}"),
            Self::ChangeNotKeptWhileRepinning => write!(formatter, "Not kept: {RepinRunning}"),
        }
    }
}
