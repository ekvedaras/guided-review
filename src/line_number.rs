use std::fmt;
use std::num::NonZeroU32;

use serde::{Deserialize, Serialize};

/// A line of a file, counted from 1. A diff line costs four bytes of it, and it cannot be mixed
/// up with a row index, an offset or a count.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct LineNumber(NonZeroU32);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidLineNumber(u64);

impl fmt::Display for InvalidLineNumber {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} is not a line number: lines count from 1 and fit 32 bits",
            self.0
        )
    }
}

impl std::error::Error for InvalidLineNumber {}

impl LineNumber {
    pub const FIRST: Self = Self(NonZeroU32::MIN);

    pub fn get(self) -> u32 {
        self.0.get()
    }
}

impl TryFrom<usize> for LineNumber {
    type Error = InvalidLineNumber;

    fn try_from(line: usize) -> Result<Self, Self::Error> {
        u32::try_from(line)
            .ok()
            .and_then(NonZeroU32::new)
            .map(Self)
            .ok_or_else(|| InvalidLineNumber(u64::try_from(line).unwrap_or(u64::MAX)))
    }
}

impl TryFrom<u32> for LineNumber {
    type Error = InvalidLineNumber;

    fn try_from(line: u32) -> Result<Self, Self::Error> {
        NonZeroU32::new(line)
            .map(Self)
            .ok_or_else(|| InvalidLineNumber(u64::from(line)))
    }
}

impl From<LineNumber> for usize {
    fn from(line: LineNumber) -> Self {
        Self::try_from(line.get()).unwrap_or(Self::MAX)
    }
}

impl fmt::Display for LineNumber {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}
