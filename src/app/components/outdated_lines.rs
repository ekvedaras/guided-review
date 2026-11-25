use gpui::{Div, ParentElement, SharedString, Styled, div, px};

use super::palette::{CODE_FONT, CONTROL_RADIUS, Colors};

/// What outdated lines were when they were written about, since the code under them shows what they are now.
pub fn before_block(colors: &Colors, label: &'static str, before: &[String]) -> Div {
    div()
        .flex()
        .flex_col()
        .gap_1()
        .child(div().text_xs().text_color(colors.muted_text).child(label))
        .child(
            div()
                .py_1()
                .px_2()
                .rounded(px(CONTROL_RADIUS))
                .bg(colors.removed_line)
                .font_family(*CODE_FONT)
                .text_sm()
                .overflow_hidden()
                .children(before.iter().map(|line| {
                    div()
                        .whitespace_nowrap()
                        .child(SharedString::from(line.clone()))
                })),
        )
}
