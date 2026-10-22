use super::step_page::{DiffLine, LineLocation};

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

    /// A row of another file cannot join: a comment has one file.
    pub fn extend_to(&mut self, row: LineLocation) {
        if row.path == self.anchor.path {
            self.head = row;
        }
    }

    /// The selected rows out of the rows a screen shows, in the order it shows them.
    /// `None` when the anchor or the head is not among them.
    pub fn resolve(&self, shown: &[DiffLine]) -> Option<SelectedLines> {
        let position = |at: &LineLocation| shown.iter().position(|row| &row.location == at);
        let (anchor, head) = (position(&self.anchor)?, position(&self.head)?);
        let (first, last) = (anchor.min(head), anchor.max(head));
        Some(SelectedLines {
            lines: shown[first..=last]
                .iter()
                .filter(|row| row.location.path == self.anchor.path)
                .cloned()
                .collect(),
        })
    }
}

/// The rows a reviewer selected, top to bottom. Commenting, asking about the lines and
/// suggesting a change all start from this.
#[derive(Debug, Clone)]
pub struct SelectedLines {
    lines: Vec<DiffLine>,
}

impl SelectedLines {
    pub fn lines(&self) -> &[DiffLine] {
        &self.lines
    }

    pub fn first(&self) -> &LineLocation {
        &self.lines[0].location
    }

    pub fn last(&self) -> &LineLocation {
        &self.lines[self.lines.len() - 1].location
    }

    pub fn contains(&self, location: &LineLocation) -> bool {
        self.lines.iter().any(|row| &row.location == location)
    }

    /// True when every row takes a comment (`DiffLine::takes_comment`).
    pub fn take_comment(&self) -> bool {
        self.lines.iter().all(DiffLine::takes_comment)
    }
}
