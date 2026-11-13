//! The focused change: the one change of the screen the change keys act on. It is a display option, not
//! saved, so the focus lives in the viewer and the window only reports clicks, which rows are in view
//! and how big the view is.

use std::ops::Range;
use std::sync::Arc;

use super::diff_display::{ChangeId, MoreContext};
use super::jump::{PaletteAction, PaletteGroup};
use super::piece_labels::{LabelSwitch, PieceLabels};
use super::rendered_view::{FileView, ViewSwitch};
use super::review::Review;
use crate::file_path::FilePath;
use crate::update::Build;

/// One change of the screen as the page shows it: the changed lines with the unchanged lines around
/// them, and what hangs under them.
#[derive(Debug, Clone)]
pub struct PageChange {
    path: FilePath,
    view: Option<ViewSwitch>,
    labels: Option<LabelSwitch>,
    pub(super) ids: Vec<ChangeId>,
    pub(super) rows: Range<usize>,
    pub(super) more_above: Option<MoreContext>,
    pub(super) more_below: Option<MoreContext>,
}

impl PageChange {
    pub(super) fn new(
        file: ChangeFile,
        ids: Vec<ChangeId>,
        rows: Range<usize>,
        more: (Option<MoreContext>, Option<MoreContext>),
    ) -> Self {
        Self {
            path: file.path,
            view: file.view,
            labels: file.labels,
            ids,
            rows,
            more_above: more.0,
            more_below: more.1,
        }
    }

    /// The file the change is in.
    pub fn path(&self) -> &FilePath {
        &self.path
    }

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

    pub(super) fn is_in_view(&self, visible: &Range<usize>) -> bool {
        self.rows.start < visible.end && visible.start < self.rows.end
    }
}

/// What the box of a change's file offers: the Code / Rendered switch and the piece labels' toggle.
pub(super) struct ChangeFile {
    pub path: FilePath,
    pub view: Option<ViewSwitch>,
    pub labels: Option<LabelSwitch>,
}

/// Where the focused change is in the window, in pixels from the top of the view: what `Down` and
/// `Up` need to tell a change the reader has not seen all of from one they have.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChangeInView {
    pub view_height: f32,
    pub top: f32,
    pub bottom: f32,
}

impl ChangeInView {
    /// How far to scroll to bring the unseen part of a change taller than the view nearer, a
    /// step that leaves a fifth of the view for the reader to keep their place. A change that fits
    /// the view is moved on from, not scrolled.
    fn unseen_scroll(self, way: Way) -> Option<f32> {
        if self.bottom - self.top <= self.view_height {
            return None;
        }
        let step = self.view_height * 4.0 / 5.0;
        match way {
            Way::Next => {
                let unseen = self.bottom - self.view_height;
                (unseen > 0.0).then(|| unseen.min(step))
            }
            Way::Previous => {
                let unseen = -self.top;
                (unseen > 0.0).then(|| -unseen.min(step))
            }
        }
    }
}

/// What a press of `Down` or `Up` asks the window to do.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ReadStep {
    /// Scroll the page by this many pixels, down when positive.
    Scroll(f32),
    /// The focus moved; show this row of the page.
    Focused(usize),
    Nothing,
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

    /// A click on this row of the page: focuses the change it is in. Says whether the focus moved.
    pub fn focus_change_at_row(&mut self, row: usize) -> bool {
        let page = Arc::clone(self.page());
        let Some(at) = page
            .changes()
            .iter()
            .position(|change| change.rows.contains(&row))
        else {
            return false;
        };
        if page.focused_index() == Some(at) {
            return false;
        }
        page.changes()
            .get(at)
            .is_some_and(|change| self.focus(change).is_some())
    }

    /// The window shows these rows of the page. A focus that is not among them goes to the first
    /// change that is. Says whether the focus moved.
    pub fn follow_scroll(&mut self, visible: Range<usize>) -> bool {
        let page = Arc::clone(self.page());
        page.change_to_focus_for_view(&visible)
            .is_some_and(|change| self.focus(change).is_some())
    }

    /// `Down`: scrolls through a focused change taller than the view until its end shows, then
    /// focuses the next change; at the last one it does nothing.
    pub fn read_down(&mut self, focused: Option<ChangeInView>) -> ReadStep {
        self.read(Way::Next, focused)
    }

    /// `Up`: like `read_down`, the other way.
    pub fn read_up(&mut self, focused: Option<ChangeInView>) -> ReadStep {
        self.read(Way::Previous, focused)
    }

    fn read(&mut self, way: Way, focused: Option<ChangeInView>) -> ReadStep {
        let page = Arc::clone(self.page());
        let Some(at) = page.focused_index() else {
            return self
                .step_focus(way)
                .map_or(ReadStep::Nothing, ReadStep::Focused);
        };
        if let Some(scroll) = focused.and_then(|view| view.unseen_scroll(way)) {
            return ReadStep::Scroll(scroll);
        }
        let target = match way {
            Way::Next => at.checked_add(1),
            Way::Previous => at.checked_sub(1),
        };
        target
            .and_then(|target| page.changes().get(target))
            .and_then(|change| self.focus(change))
            .map_or(ReadStep::Nothing, ReadStep::Focused)
    }

    /// What the palette can do on the open screen, with the toggles of the focused change's file.
    pub fn palette_actions(&self, build: Build) -> Vec<PaletteAction> {
        self.palette_actions_with(build, self.file_toggle_actions())
    }

    /// The cmd+k palette for what the reviewer has typed so far: places by kind, then actions.
    pub fn palette(&self, query: &str, build: Build) -> Vec<PaletteGroup> {
        self.palette_with(query, build, self.file_toggle_actions())
    }

    fn file_toggle_actions(&self) -> Vec<PaletteAction> {
        let Some(change) = self.page().focused_change() else {
            return Vec::new();
        };
        let view = change.view.as_ref().map(|switch| match switch.view {
            FileView::Code => PaletteAction::ShowRendered,
            FileView::Rendered => PaletteAction::ShowCode,
        });
        let labels = change.labels.as_ref().map(|switch| match switch.labels {
            PieceLabels::Shown => PaletteAction::HidePieceLabels,
            PieceLabels::Hidden => PaletteAction::ShowPieceLabels,
        });
        labels.into_iter().chain(view).collect()
    }

    /// `L`: shows or hides the piece labels of the focused change's file. Says whether it did: not
    /// with no focused change, nor in a file whose box has no labels.
    pub fn toggle_focused_piece_labels(&mut self) -> bool {
        let Some(switch) = self
            .page()
            .focused_change()
            .and_then(|change| change.labels.clone())
        else {
            return false;
        };
        self.display_options_mut()
            .show_piece_labels(&switch.path, switch.labels.flipped());
        true
    }

    /// `R`: switches the focused change's file between Code and Rendered. Says whether it did: not
    /// with no focused change, nor in a file with no Rendered view.
    pub fn toggle_focused_file_view(&mut self) -> bool {
        let Some(switch) = self
            .page()
            .focused_change()
            .and_then(|change| change.view.clone())
        else {
            return false;
        };
        let view = match switch.view {
            FileView::Code => FileView::Rendered,
            FileView::Rendered => FileView::Code,
        };
        self.display_options_mut().show_file_as(&switch.path, view);
        true
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
