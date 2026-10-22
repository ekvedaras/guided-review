use gpui::{AnyElement, App, IntoElement, ParentElement, RenderOnce, Styled, Window, div};

use crate::viewer::{LineLocation, SafetyNetPage};

use super::diff_view::{DiffView, OnMoreContext, OnOpenInIde, RowSelecting, threads_by_piece};
use super::palette::Palette;

/// Every change the tour does not show, so nothing in the PR stays hidden.
#[derive(IntoElement)]
pub struct SafetyNetView {
    page: SafetyNetPage,
    on_more_context: OnMoreContext,
    line_threads: Vec<(LineLocation, AnyElement)>,
    other_threads: Vec<AnyElement>,
    selecting: Option<RowSelecting>,
    open_in_ide: Option<OnOpenInIde>,
    /// One per piece, `Some` for the pieces that are pictures.
    image_rows: Vec<Option<AnyElement>>,
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
            open_in_ide: None,
            image_rows: Vec::new(),
        }
    }

    pub fn with_open_in_ide(mut self, open_in_ide: Option<OnOpenInIde>) -> Self {
        self.open_in_ide = open_in_ide;
        self
    }

    pub fn with_image_rows(mut self, image_rows: Vec<Option<AnyElement>>) -> Self {
        self.image_rows = image_rows;
        self
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
                    .zip(
                        self.image_rows
                            .into_iter()
                            .chain(std::iter::repeat_with(|| None)),
                    )
                    .map(|((threads, piece), image_row)| {
                        let view = DiffView::new(piece, self.on_more_context.clone())
                            .with_image_row(image_row)
                            .with_threads(threads)
                            .with_open_in_ide(self.open_in_ide.clone());
                        match self.selecting.clone() {
                            Some(selecting) => view.with_row_selecting(selecting),
                            None => view,
                        }
                    }),
            )
            .children(self.other_threads)
    }
}
