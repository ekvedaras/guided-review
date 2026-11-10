use gpui::{App, FontWeight, IntoElement, ParentElement, RenderOnce, Styled, Window, div};

use super::palette::Colors;

/// The top of the safety net: every change the tour does not show, so nothing in the PR stays hidden.
/// Its pieces and threads follow as rows of the screen's list.
#[derive(IntoElement)]
pub struct SafetyNetIntro;

impl SafetyNetIntro {
    pub const TITLE: &str = "Not in any step";
}

impl RenderOnce for SafetyNetIntro {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let colors = Colors::active(cx);
        div()
            .flex()
            .flex_col()
            .gap_2()
            .pb_5()
            .child(
                div()
                    .text_2xl()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(Self::TITLE),
            )
            .child(
                div()
                    .text_color(colors.muted_text)
                    .child("Changes in this PR that no step of the tour shows."),
            )
    }
}
