use std::fmt;

use crate::findings::FindingId;
use crate::viewer::comments::CommentRefusal;

/// Why an action on a finding did not happen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FindingRefusal {
    /// The finding is not among the open ones: it was decided already, or a new run replaced it.
    NotThere,
    /// The decision could not be kept.
    NotKept(String),
    /// A comment box with text is open on those lines: sending the finding would send that text unread.
    DraftInTheWay,
    /// A fix is asked for already, or under way: asking again would send it twice.
    BeingFixed,
    /// A question on it waits for its answer: another would cross it.
    BeingAsked,
    Comment(CommentRefusal),
}

impl fmt::Display for FindingRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotThere => f.write_str("This finding is not open any more"),
            Self::NotKept(reason) => write!(f, "The decision could not be kept: {reason}"),
            Self::DraftInTheWay => {
                f.write_str("A comment box is open on these lines: use the finding there, or send that comment first")
            }
            Self::BeingFixed => f.write_str("A fix of this finding is asked for already"),
            Self::BeingAsked => {
                f.write_str("A question on this finding waits for the review agent's answer")
            }
            Self::Comment(refusal) => refusal.fmt(f),
        }
    }
}

/// What a batch did: what went through, and each finding that did not, with the reason.
#[derive(Debug)]
pub struct FindingsBatch<T> {
    pub done: Vec<T>,
    pub refused: Vec<(FindingId, FindingRefusal)>,
}
