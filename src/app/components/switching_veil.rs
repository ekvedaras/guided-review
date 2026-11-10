use gpui::{
    App, FontWeight, InteractiveElement, IntoElement, ParentElement, RenderOnce, SharedString,
    Styled, Window, div, px,
};

use super::motion::{STATE, busy_dot, busy_sweep, fade_in, rise_in};
use super::palette::{CARD_RADIUS, Colors, TEXT_FONT};
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
        let panel = div()
            .min_w(px(360.))
            .px_6()
            .pt_5()
            .pb_6()
            .flex()
            .flex_col()
            .gap_4()
            .rounded(px(CARD_RADIUS))
            .border_1()
            .border_color(colors.border)
            .bg(colors.overlay)
            .shadow(colors.overlay_shadow())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(busy_dot(&colors, "veil-dot"))
                    .child(
                        div()
                            .text_color(colors.text)
                            .text_lg()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(self.label),
                    ),
            )
            .child(busy_sweep(&colors, "veil-sweep"));
        fade_in(
            div()
                .absolute()
                .top(px(TITLE_BAR_HEIGHT))
                .left_0()
                .right_0()
                .bottom_0()
                .flex()
                .items_center()
                .justify_center()
                .bg(colors.scrim)
                .occlude()
                .font_family(TEXT_FONT),
            "switching-veil",
            STATE,
        )
        .child(rise_in(panel, "switching-veil-panel"))
    }
}
