use std::fmt::Write;

use super::comments::{CommentPlace, CommentRefusal};
use super::diff_line::DiffLine;
use super::line_location::{LineLocation, Side};

/// A drag over rows of one file: where the mouse went down and where it is now.
/// The rows in between belong to it, whatever hunk or side they sit in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowSelection {
    anchor: LineLocation,
    head: LineLocation,
}

impl RowSelection {
    pub fn start_at(row: LineLocation) -> Self {
        Self {
            anchor: row.clone(),
            head: row,
        }
    }

    /// Where the mouse went down.
    pub fn anchor(&self) -> &LineLocation {
        &self.anchor
    }

    /// Where the mouse is now.
    pub fn head(&self) -> &LineLocation {
        &self.head
    }

    /// The rows of a comment on lines, from its first to its last; `None` for a comment on a file.
    pub fn of(place: &CommentPlace) -> Option<Self> {
        let CommentPlace::Lines { start, end } = place else {
            return None;
        };
        let mut selection = Self::start_at(start.clone());
        selection.extend_to(end.clone());
        Some(selection)
    }

    /// A row of another file cannot join: a comment has one file.
    pub fn extend_to(&mut self, row: LineLocation) {
        if row.path == self.anchor.path {
            self.head = row;
        }
    }

    /// The selected rows out of the rows a screen shows, in the order it shows them.
    /// `None` when the anchor or the head is not among them.
    pub fn resolve<'a>(
        &self,
        shown: impl Iterator<Item = &'a DiffLine> + Clone,
    ) -> Option<SelectedLines> {
        let position = |at: &LineLocation| shown.clone().position(|row| &row.location == at);
        let (anchor, head) = (position(&self.anchor)?, position(&self.head)?);
        let (first, last) = (anchor.min(head), anchor.max(head));
        SelectedLines::new(
            shown
                .skip(first)
                .take(last - first + 1)
                .filter(|row| row.location.path == self.anchor.path)
                .cloned()
                .collect(),
        )
    }
}

/// The rows a reviewer selected, top to bottom. Commenting, asking about the lines and
/// suggesting a change all start from this.
#[derive(Debug, Clone)]
pub struct SelectedLines {
    lines: Vec<DiffLine>,
    first: LineLocation,
    last: LineLocation,
}

impl SelectedLines {
    /// The place a comment on these lines has.
    pub fn comment_place(&self) -> CommentPlace {
        CommentPlace::Lines {
            start: self.first().clone(),
            end: self.last().clone(),
        }
    }

    /// `None` for no rows, so a selection always has a first and a last.
    pub(super) fn new(lines: Vec<DiffLine>) -> Option<Self> {
        let first = lines.first()?.location.clone();
        let last = lines.last()?.location.clone();
        Some(Self { lines, first, last })
    }

    pub fn lines(&self) -> &[DiffLine] {
        &self.lines
    }

    /// The text of the rows on the head side, in order: what a suggestion replaces the lines with.
    pub(super) fn head_texts(&self) -> Vec<&str> {
        self.lines
            .iter()
            .filter(|row| row.location.side == Side::Head)
            .map(|row| row.text.as_str())
            .collect()
    }

    /// The rows as an agent is told them, one per line: `+ path:side:line: text`.
    pub(super) fn told_to_an_agent(&self) -> String {
        let mut told = String::new();
        for row in &self.lines {
            let (sign, side) = match row.location.side {
                Side::Base => ('-', "base"),
                Side::Head => ('+', "head"),
            };
            writeln!(
                told,
                "{sign} {}:{side}:{}: {}",
                row.location.path, row.location.line, row.text
            )
            .ok();
        }
        told
    }

    /// Each row's line number and text, whichever side it is on.
    pub(super) fn numbered_texts(&self) -> impl Iterator<Item = (u32, &str)> {
        self.lines
            .iter()
            .map(|row| (row.location.line.get(), row.text.as_str()))
    }

    pub fn first(&self) -> &LineLocation {
        &self.first
    }

    pub fn last(&self) -> &LineLocation {
        &self.last
    }

    pub fn contains(&self, location: &LineLocation) -> bool {
        self.lines.iter().any(|row| &row.location == location)
    }

    /// Why the selection takes no comment: the first row the host takes none on says so.
    pub fn comment_refusal(&self) -> Option<CommentRefusal> {
        self.lines.iter().find_map(DiffLine::comment_refusal)
    }

    /// True when every row takes a comment (`DiffLine::takes_comment`).
    pub fn take_comment(&self) -> bool {
        self.comment_refusal().is_none()
    }
}
