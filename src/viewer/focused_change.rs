//! The focused change: the one change of the screen the change keys act on. It is a display option, not
//! saved, so the focus lives in the viewer and the window only reports clicks, which rows are in view
//! and how big the view is.

use std::ops::Range;
use std::sync::Arc;

use super::diff_display::{ChangeId, MoreContext};
use super::review::Review;

/// One change of the screen as the page shows it: the changed lines with the unchanged lines around
/// them, and what hangs under them.
#[derive(Debug, Clone)]
pub struct PageChange {
    pub(super) ids: Vec<ChangeId>,
    pub(super) rows: Range<usize>,
    pub(super) more_above: Option<MoreContext>,
    pub(super) more_below: Option<MoreContext>,
}

impl PageChange {
    /// The page rows it spans, first to last.
    pub fn rows(&self) -> Range<usize> {
        self.rows.clone()
    }

    pub fn first_row(&self) -> usize {
        self.rows.start
    }

    pub(super) fn holds(&self, id: &ChangeId) -> bool {
        self.ids.contains(id)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Way {
    Next,
    Previous,
}

impl Review {
    /// Moves the focus to the next change of the screen, the first when none has it and from the
    /// last back to the first. Returns the row the window shows it at; `None` on a screen with no code.
    pub fn focus_next_change(&mut self) -> Option<usize> {
        self.step_focus(Way::Next)
    }

    /// Like `focus_next_change`, the other way; from none or the first it goes to the last.
    pub fn focus_previous_change(&mut self) -> Option<usize> {
        self.step_focus(Way::Previous)
    }

    /// Shows the same lines the gap button over the focused change shows. Says whether the change
    /// has focus and the file has lines to show there.
    pub fn show_more_above(&mut self) -> bool {
        self.show_more(|change| change.more_above.clone())
    }

    /// Shows the same lines the gap button under the focused change shows.
    pub fn show_more_below(&mut self) -> bool {
        self.show_more(|change| change.more_below.clone())
    }

    fn show_more(&mut self, edge: impl FnOnce(&PageChange) -> Option<MoreContext>) -> bool {
        let Some(more) = self.page().focused_change().and_then(edge) else {
            return false;
        };
        self.display_options_mut().show_more_context(&more);
        true
    }

    fn step_focus(&mut self, way: Way) -> Option<usize> {
        let page = Arc::clone(self.page());
        let count = page.changes().len();
        let target = match (way, page.focused_index()) {
            (Way::Next, Some(at)) => (at + 1) % count,
            (Way::Next, None) => 0,
            (Way::Previous, Some(at)) => {
                at.checked_sub(1).unwrap_or_else(|| count.saturating_sub(1))
            }
            (Way::Previous, None) => count.checked_sub(1)?,
        };
        self.focus(page.changes().get(target)?)
    }

    fn focus(&mut self, change: &PageChange) -> Option<usize> {
        let id = change.ids.first()?.clone();
        self.display_options_mut().focus_change(id);
        Some(change.first_row())
    }
}
