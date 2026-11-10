use gpui::prelude::FluentBuilder;
use gpui::{
    AnyElement, Div, ElementId, FontWeight, InteractiveElement, IntoElement, ParentElement, Pixels,
    Stateful, StatefulInteractiveElement, Styled, div, px,
};

use super::icon::{Icon, sized_icon};
use super::motion::pop_when_shown;
use super::palette::{CONTROL_RADIUS, Colors};

const FLAT_INDENT: f32 = 8.;
const INDENT_PER_DEPTH: f32 = 16.;
/// One line of `text_sm`, so a mark sits on the first line of a label that wraps.
const LINE_HEIGHT: f32 = 20.;
const MARK_SIZE: f32 = 14.;

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
    colors: &Colors,
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
        .py(px(3.))
        .flex()
        .items_start()
        .gap_2()
        .rounded(px(CONTROL_RADIUS))
        .border_1()
        .cursor_pointer()
        .text_color(match mark {
            RowMark::Read | RowMark::Gone if !is_current => colors.muted_text,
            _ => colors.text,
        })
        .map(|row| {
            if is_current {
                row.bg(colors.current)
                    .border_color(colors.current_border)
                    .font_weight(FontWeight::MEDIUM)
            } else {
                row.border_color(gpui::transparent_black())
                    .hover(|row| row.bg(colors.hover))
                    .active(|row| row.bg(colors.pressed))
            }
        })
        .child(mark_element(colors, mark, is_current))
}

/// The current step wears the trail's dot; elsewhere a ring, a check or a dash says what was read.
/// The check is always there and only drawn at size once read, so it pops in at the moment of the
/// tick and not when the window opens.
fn mark_element(colors: &Colors, mark: RowMark, is_current: bool) -> AnyElement {
    let slot = div()
        .relative()
        .flex_none()
        .w(px(MARK_SIZE))
        .h(px(LINE_HEIGHT))
        .flex()
        .items_center()
        .justify_center();
    if mark == RowMark::Page {
        return slot.into_any_element();
    }
    let shows_check = mark == RowMark::Read && !is_current;
    let check = div()
        .absolute()
        .inset_0()
        .flex()
        .items_center()
        .justify_center()
        .child(pop_when_shown(
            sized_icon(Icon::Check, colors.success, px(MARK_SIZE)),
            "read-mark",
            shows_check,
        ));
    let under = if is_current {
        Some(trail_dot(colors).into_any_element())
    } else {
        match mark {
            RowMark::Unread => Some(
                div()
                    .size(px(8.))
                    .rounded_full()
                    .border_1()
                    .border_color(colors.muted_text)
                    .into_any_element(),
            ),
            RowMark::Gone => {
                Some(sized_icon(Icon::Minus, colors.muted_text, px(MARK_SIZE)).into_any_element())
            }
            RowMark::Read | RowMark::Page => None,
        }
    };
    slot.children(under).child(check).into_any_element()
}

/// Where the reader is on the tour: a trail dot in a soft disc of the trail's colour.
fn trail_dot(colors: &Colors) -> impl IntoElement {
    div()
        .size(px(MARK_SIZE))
        .rounded_full()
        .bg(colors.trail_glow())
        .flex()
        .items_center()
        .justify_center()
        .child(
            div()
                .size(px(8.))
                .rounded_full()
                .bg(colors.trail.left_right()),
        )
}

/// How many review threads hang on a row's screen.
pub fn count_badge(colors: &Colors, count: usize) -> Option<impl IntoElement> {
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
                    .border_color(colors.subtle_border)
                    .text_xs()
                    .text_color(colors.muted_text)
                    .child(count.to_string()),
            )
    })
}

fn indent(depth: usize) -> Pixels {
    px(FLAT_INDENT + INDENT_PER_DEPTH * depth as f32)
}
