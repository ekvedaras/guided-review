use gpui::{
    App, FontWeight, InteractiveElement, IntoElement, ParentElement, RenderOnce, SharedString,
    Styled, Window, div, px,
};

use super::palette::{Colors, TEXT_FONT};
use super::title_bar::TITLE_BAR_HEIGHT;
use crate::pull_request::PullRequestUrl;

/// Dims the PR on screen while the picked one opens, so the window plainly says it is busy.
#[derive(IntoElement)]
pub struct SwitchingVeil {
    label: SharedString,
}

impl SwitchingVeil {
    pub fn to(pr: &PullRequestUrl) -> Self {
        Self {
            label: format!("Opening {}...", pr.short()).into(),
        }
    }
}

impl RenderOnce for SwitchingVeil {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let colors = Colors::active(cx);
        div()
            .absolute()
            .top(px(TITLE_BAR_HEIGHT))
            .left_0()
            .right_0()
            .bottom_0()
            .flex()
            .items_center()
            .justify_center()
            .bg(gpui::rgba(0x00000066))
            .occlude()
            .font_family(TEXT_FONT)
            .child(
                div()
                    .px_6()
                    .py_4()
                    .rounded_md()
                    .border_1()
                    .border_color(colors.border)
                    .bg(colors.surface)
                    .text_color(colors.text)
                    .text_xl()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(self.label),
            )
    }
}
