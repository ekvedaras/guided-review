//! One row of a diff as a screen shows it: its text, its place and whether it takes a comment.

use super::code_link::CodeLink;
use super::comments::CommentRefusal;
use super::highlight::Span;
use super::line_location::{LineLocation, Side};
use crate::line_number::LineNumber;
use std::ops::Range;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineKind {
    Removed,
    Added,
    /// Unchanged: its location is at head, and this is where it is at base. `None` when the
    /// hunks above it do not add up to a line, as in a diff that does not match the file.
    Context {
        base_line: Option<LineNumber>,
        /// Shown on the reviewer's ask, outside GitHub's diff, so GitHub takes no comment on it.
        is_expanded: bool,
    },
}

/// Which diff a row is drawn in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowView {
    /// The PR's diff against its base.
    PrDiff,
    /// The "change since your read" view, whose removed rows are lines of the head the step was
    /// read at, not of the PR's base. Its head rows are lines of the PR's head.
    ChangeSinceRead { head_line_is_in_pr_diff: bool },
}

#[derive(Debug, Clone)]
pub struct DiffLine {
    pub location: LineLocation,
    pub kind: LineKind,
    pub text: String,
    /// Empty until the file's highlights are ready.
    pub spans: Vec<Span>,
    /// Opened through the `CodeOpener`.
    pub code_link: CodeLink,
    pub view: RowView,
    /// Byte ranges of the words that changed, on a removed or added line that has a partner on the
    /// other side; empty when the whole line changed.
    pub changed: Vec<Range<usize>>,
}

impl DiffLine {
    /// A thread hangs under this row when it is on the row's own line, or, for an unchanged row,
    /// on the line it has at base.
    pub(super) fn holds(&self, location: &LineLocation) -> bool {
        self.takes_comment() && self.shows(location)
    }

    /// Whether this row is `location`: its own line, or for an unchanged row, the line it has at base.
    pub(super) fn shows(&self, location: &LineLocation) -> bool {
        self.location.path == location.path
            && match (self.kind, location.side) {
                (LineKind::Context { base_line, .. }, Side::Base) => {
                    base_line == Some(location.line)
                }
                _ => self.location.side == location.side && self.location.line == location.line,
            }
    }

    pub fn is_change(&self) -> bool {
        matches!(self.kind, LineKind::Removed | LineKind::Added)
    }

    /// Only lines inside a GitHub diff hunk can take a comment.
    pub fn is_inside_hunk(&self) -> bool {
        !matches!(
            self.kind,
            LineKind::Context {
                is_expanded: true,
                ..
            }
        )
    }
}

impl DiffLine {
    /// The one rule for a comment button and for a selection: why GitHub takes no comment on this
    /// row, or `None` when it does. A head line keeps its number when it is commented on from the
    /// change since the read, because that view's head is the PR's head.
    pub fn comment_refusal(&self) -> Option<CommentRefusal> {
        match (self.view, self.location.side) {
            (RowView::PrDiff, _) => {
                (!self.is_inside_hunk()).then_some(CommentRefusal::NotInsideHunk)
            }
            (RowView::ChangeSinceRead { .. }, Side::Base) => Some(CommentRefusal::OlderHeadLine),
            (
                RowView::ChangeSinceRead {
                    head_line_is_in_pr_diff,
                },
                Side::Head,
            ) => (!head_line_is_in_pr_diff).then_some(CommentRefusal::NotInsideHunk),
        }
    }

    pub fn takes_comment(&self) -> bool {
        self.comment_refusal().is_none()
    }

    /// A row shows the comment button when it could be a line of a GitHub diff, so that pressing
    /// it can say why it takes no comment.
    pub fn offers_comment_button(&self) -> bool {
        match self.view {
            RowView::PrDiff => self.is_inside_hunk(),
            RowView::ChangeSinceRead { .. } => true,
        }
    }
}
