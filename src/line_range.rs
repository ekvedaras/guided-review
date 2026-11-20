use std::fmt;

use serde::{Deserialize, Serialize};

use crate::line_number::LineNumber;

/// The lines from `start` to `end` of one side of a file, both included. It is written as a pair of
/// line numbers, `[start, end]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(from = "[LineNumber; 2]", into = "[LineNumber; 2]")]
pub struct LineRange {
    pub start: LineNumber,
    pub end: LineNumber,
}

impl LineRange {
    pub const fn new(start: LineNumber, end: LineNumber) -> Self {
        Self { start, end }
    }

    pub fn is_single(self) -> bool {
        self.start == self.end
    }
}

impl From<[LineNumber; 2]> for LineRange {
    fn from([start, end]: [LineNumber; 2]) -> Self {
        Self::new(start, end)
    }
}

impl From<LineRange> for [LineNumber; 2] {
    fn from(range: LineRange) -> Self {
        [range.start, range.end]
    }
}

impl fmt::Display for LineRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_single() {
            write!(f, "line {}", self.start)
        } else {
            write!(f, "lines {} to {}", self.start, self.end)
        }
    }
}
