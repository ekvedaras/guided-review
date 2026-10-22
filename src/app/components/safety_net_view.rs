use gpui::{App, IntoElement, ParentElement, RenderOnce, Styled, Window, div};

use crate::viewer::SafetyNetPage;

use super::diff_view::DiffView;
use super::palette::Palette;

/// Every change the tour does not show, so nothing in the PR stays hidden.
#[derive(IntoElement)]
pub struct SafetyNetView {
    page: SafetyNetPage,
}

impl SafetyNetView {
    pub const TITLE: &str = "Not in any step";

    pub fn new(page: SafetyNetPage) -> Self {
        Self { page }
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
            .children(self.page.pieces.into_iter().map(DiffView::new))
    }
}
