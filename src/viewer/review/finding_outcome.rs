use std::fmt;

use crate::findings::FindingId;
use crate::viewer::comments::{CommentPlace, CommentRefusal};

/// Why an action on a finding did not happen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FindingRefusal {
    /// The finding is not among the open ones: it was decided already, or a new run replaced it.
    NotThere,
    /// The finding is not among the dismissed ones: it was restored already, or never dismissed.
    NotDismissed,
    /// An open finding has the id of the dismissed one: a later run reused it.
    IdTaken,
    /// The findings file is gone or unreadable, so there is no list to put the finding back in.
    NoFindingsFile,
    /// The decision could not be kept.
    NotKept(String),
    /// A comment box is open on those lines, at that place: sending the finding would send its text unread.
    DraftInTheWay(CommentPlace),
    /// A fix is asked for already, or under way: asking again would send it twice.
    BeingFixed,
    /// Mark fixed is for a finding the tour agent said it fixed.
    NotFixedYet,
    /// It was not added to fixes, or it was sent already.
    NotInFixes,
    /// A question on it waits for its answer: another would cross it.
    BeingAsked,
    /// The tour agent asked nothing of the reviewer on this finding, or it was answered already.
    NoFixQuestion,
    /// The review agent that reported the finding is gone, and the finding sits on no lines of the diff,
    /// so there is nothing to ask the tour agent on.
    NoReviewAgent,
    Comment(CommentRefusal),
}

impl fmt::Display for FindingRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotThere => f.write_str("This finding is not open any more"),
            Self::NotDismissed => f.write_str("This finding is not dismissed any more"),
            Self::IdTaken => f.write_str("A finding with the same id is open already"),
            Self::NoFindingsFile => f.write_str("The review has no findings to put it back among"),
            Self::NotKept(reason) => write!(f, "The decision could not be kept: {reason}"),
            Self::DraftInTheWay(_) => f.write_str(
                "A comment box is open on these lines: Use adds the finding to it, or send that comment first",
            ),
            Self::BeingFixed => f.write_str("A fix of this finding is asked for already"),
            Self::NotFixedYet => f.write_str("The tour agent has not fixed this finding"),
            Self::NotInFixes => f.write_str("This finding is not among the fixes to send"),
            Self::BeingAsked => {
                f.write_str("A question on this finding waits for the review agent's answer")
            }
            Self::NoFixQuestion => {
                f.write_str("The tour agent has no question on this finding to answer")
            }
            Self::NoReviewAgent => f.write_str("The review agent that reported this is gone, and no lines of the diff hold it to ask your agent on"),
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
