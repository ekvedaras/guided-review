use gpui::prelude::FluentBuilder;
use gpui::{
    Div, ElementId, FontWeight, InteractiveElement, IntoElement, ParentElement, Pixels, Stateful,
    Styled, div, px,
};

use super::palette::Colors;

const FLAT_INDENT: f32 = 8.;
const INDENT_PER_DEPTH: f32 = 16.;
/// One line of `text_sm`, so a mark sits on the first line of a label that wraps.
const LINE_HEIGHT: f32 = 20.;

/// What the reader has done with the screen a row opens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowMark {
    Unread,
    Read,
    /// Every piece of the step went away in a re-pin.
    Gone,
    /// The summary and the safety net are not ticked.
    Page,
}

/// The look every clickable row of the sidebar shares: a mark, then the caller's label.
pub fn sidebar_row(
    colors: Colors,
    id: impl Into<ElementId>,
    depth: usize,
    is_current: bool,
    mark: RowMark,
) -> Stateful<Div> {
    div()
        .id(id)
        .mx_2()
        .pl(indent(depth))
        .pr_2()
        .py_1()
        .flex()
        .items_start()
        .gap_2()
        .rounded_md()
        .cursor_pointer()
        .text_color(match mark {
            RowMark::Read | RowMark::Gone if !is_current => colors.muted_text,
            _ => colors.text,
        })
        .map(|row| {
            if is_current {
                row.bg(colors.current).font_weight(FontWeight::MEDIUM)
            } else {
                row.hover(|row| row.bg(colors.hover))
            }
        })
        .child(mark_element(colors, mark, is_current))
}

fn mark_element(colors: Colors, mark: RowMark, is_current: bool) -> impl IntoElement {
    let slot = div()
        .flex_none()
        .w(px(14.))
        .h(px(LINE_HEIGHT))
        .flex()
        .items_center()
        .justify_center();
    match mark {
        RowMark::Unread => slot.child(div().size(px(8.)).rounded_full().border_1().border_color(
            if is_current {
                colors.accent
            } else {
                colors.muted_text
            },
        )),
        RowMark::Read => slot.text_color(colors.success).text_xs().child("✓"),
        RowMark::Gone => slot.text_color(colors.muted_text).child("–"),
        RowMark::Page => slot,
    }
}

/// How many review threads hang on a row's screen.
pub fn count_badge(colors: Colors, count: usize) -> Option<impl IntoElement> {
    (count > 0).then(|| {
        div()
            .flex_none()
            .h(px(LINE_HEIGHT))
            .flex()
            .items_center()
            .child(
                div()
                    .px_1p5()
                    .rounded_full()
                    .bg(colors.surface)
                    .border_1()
                    .border_color(colors.border)
                    .text_xs()
                    .text_color(colors.muted_text)
                    .child(count.to_string()),
            )
    })
}

fn indent(depth: usize) -> Pixels {
    px(FLAT_INDENT + INDENT_PER_DEPTH * depth as f32)
}
