use gpui::{AnyElement, App, IntoElement, ParentElement, RenderOnce, Styled, Window, div};

use crate::viewer::{LineLocation, SafetyNetPage};

use super::diff_view::{DiffView, OnMoreContext, RowSelecting, threads_by_piece};
use super::palette::Palette;

/// Every change the tour does not show, so nothing in the PR stays hidden.
#[derive(IntoElement)]
pub struct SafetyNetView {
    page: SafetyNetPage,
    on_more_context: OnMoreContext,
    line_threads: Vec<(LineLocation, AnyElement)>,
    other_threads: Vec<AnyElement>,
    selecting: Option<RowSelecting>,
}

impl SafetyNetView {
    pub const TITLE: &str = "Not in any step";

    pub fn new(page: SafetyNetPage, on_more_context: OnMoreContext) -> Self {
        Self {
            page,
            on_more_context,
            line_threads: Vec::new(),
            other_threads: Vec::new(),
            selecting: None,
        }
    }

    pub fn with_row_selecting(mut self, selecting: RowSelecting) -> Self {
        self.selecting = Some(selecting);
        self
    }

    pub fn with_threads(
        mut self,
        line_threads: Vec<(LineLocation, AnyElement)>,
        other_threads: Vec<AnyElement>,
    ) -> Self {
        self.line_threads = line_threads;
        self.other_threads = other_threads;
        self
    }
}

impl RenderOnce for SafetyNetView {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(div().text_xl().child(Self::TITLE))
            .child(
                div()
                    .text_color(Palette::muted_text())
                    .child("Changes in this PR that no step of the tour shows."),
            )
            .children(
                threads_by_piece(&self.page.pieces, self.line_threads)
                    .into_iter()
                    .zip(self.page.pieces)
                    .map(|(threads, piece)| {
                        let view = DiffView::new(piece, self.on_more_context.clone())
                            .with_threads(threads);
                        match self.selecting.clone() {
                            Some(selecting) => view.with_row_selecting(selecting),
                            None => view,
                        }
                    }),
            )
            .children(self.other_threads)
    }
}
