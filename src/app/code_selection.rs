//! The code text the reader drags over to copy, as rows of the screen's list and byte offsets in
//! their code. A looks-only state of the layout, like the drag that opens a comment.

use std::ops::Range;

use crate::viewer::Screen;

/// A spot in the code of one list row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct CodeSpot {
    pub row: usize,
    pub byte: usize,
}

#[derive(Debug, Clone)]
pub struct CodeSelection {
    /// Rows are of this screen; another screen has other rows under the same numbers.
    screen: Screen,
    anchor: CodeSpot,
    head: CodeSpot,
    is_dragging: bool,
}

impl CodeSelection {
    pub fn start(screen: Screen, at: CodeSpot) -> Self {
        Self {
            screen,
            anchor: at,
            head: at,
            is_dragging: true,
        }
    }

    pub fn drag_to(&mut self, at: CodeSpot) {
        if self.is_dragging {
            self.head = at;
        }
    }

    /// `false` when the press never moved: a click selects nothing.
    pub fn end_drag(&mut self) -> bool {
        self.is_dragging = false;
        self.anchor != self.head
    }

    pub fn is_dragging(&self) -> bool {
        self.is_dragging
    }

    pub fn is_on(&self, screen: &Screen) -> bool {
        &self.screen == screen
    }

    fn ends(&self) -> (CodeSpot, CodeSpot) {
        if self.anchor <= self.head {
            (self.anchor, self.head)
        } else {
            (self.head, self.anchor)
        }
    }

    /// The selected bytes of a row's code of `length` bytes; rows between the ends are whole.
    pub fn range_on(&self, row: usize, length: usize) -> Option<Range<usize>> {
        let (start, end) = self.ends();
        if row < start.row || row > end.row {
            return None;
        }
        let from = if row == start.row { start.byte } else { 0 };
        let to = if row == end.row { end.byte } else { length };
        let range = from.min(length)..to.min(length);
        (!range.is_empty() || (row != end.row && length == 0)).then_some(range)
    }

    /// The selected text, a line per row; `code_of` gives a row's code, `None` for rows without code.
    /// An end inside a letter takes the whole letter.
    pub fn text<'a>(&self, code_of: impl Fn(usize) -> Option<&'a str>) -> String {
        let (start, end) = self.ends();
        (start.row..=end.row)
            .filter_map(|row| {
                let code = code_of(row)?;
                let range = self.range_on(row, code.len()).unwrap_or(0..0);
                code.get(code.floor_char_boundary(range.start)..code.ceil_char_boundary(range.end))
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spot(row: usize, byte: usize) -> CodeSpot {
        CodeSpot { row, byte }
    }

    fn dragged(from: CodeSpot, to: CodeSpot) -> CodeSelection {
        let mut selection = CodeSelection::start(Screen::Summary, from);
        selection.drag_to(to);
        selection.end_drag();
        selection
    }

    #[test]
    fn a_drag_over_rows_copies_the_first_line_end_the_middle_lines_and_the_last_line_start() {
        let code = ["let a = 1;", "let b = 2;", "let c = 3;"];
        let selection = dragged(spot(2, 5), spot(0, 4));

        assert_eq!(
            selection.text(|row| code.get(row).copied()),
            "a = 1;\nlet b = 2;\nlet c"
        );
    }

    #[test]
    fn a_drag_ending_inside_a_letter_copies_the_whole_letter_and_keeps_the_row() {
        let code = ["ąb", "cė"];
        let selection = dragged(spot(0, 1), spot(1, 2));

        assert_eq!(selection.text(|row| code.get(row).copied()), "ąb\ncė");
    }

    #[test]
    fn rows_without_code_are_skipped_in_the_copy() {
        let code = [Some("one"), None, Some("three")];
        let selection = dragged(spot(0, 0), spot(2, 5));

        assert_eq!(selection.text(|row| code[row]), "one\nthree");
    }

    #[test]
    fn a_click_that_never_moved_selects_nothing() {
        let mut selection = CodeSelection::start(Screen::Summary, spot(3, 2));

        assert!(!selection.end_drag());
    }

    #[test]
    fn after_the_mouse_is_let_go_moving_it_changes_nothing() {
        let mut selection = dragged(spot(0, 0), spot(0, 3));
        selection.drag_to(spot(5, 1));

        assert_eq!(selection.range_on(0, 10), Some(0..3));
        assert_eq!(selection.range_on(5, 10), None);
    }
}
