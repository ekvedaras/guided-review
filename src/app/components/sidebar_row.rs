use gpui::prelude::FluentBuilder;
use gpui::{
    AnyElement, Div, ElementId, FontWeight, InteractiveElement, IntoElement, ParentElement, Pixels,
    SharedString, Stateful, StatefulInteractiveElement, Styled, div, phi, px,
};

use super::chip::chip;
use super::icon::{Icon, sized_icon};
use super::motion::pop_when_shown;
use super::palette::{CONTROL_RADIUS, Colors, Tone};
use super::tooltip::tooltip;

const FLAT_INDENT: f32 = 8.;
const INDENT_PER_DEPTH: f32 = 16.;
/// A row's line height, set on the row itself, so the marks, chips and badges beside a label centre
/// on its first line even when it wraps.
const LINE_HEIGHT: f32 = 22.;
const MARK_SIZE: f32 = 14.;
/// The row's inset at top, bottom and right, so a mark in its corner is as far from the top edge as
/// from the right. Text keeps `TEXT_END` more room at the right.
const ROW_INSET: f32 = 3.;
const TEXT_END: f32 = 5.;
/// A pill in the corner, like `skim`, keeps the room beside it that its line leaves above it: a
/// `text_xs` pill is 12 px times phi high in a `LINE_HEIGHT` line.
const PILL_END: f32 = (LINE_HEIGHT - 12. * 1.618) / 2.;

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

/// The look every clickable row of the sidebar shares: a mark, then the caller's label. The current row squares
/// the corners it shares with `hovered_touching`, the touching row under the mouse.
pub fn sidebar_row(
    colors: &Colors,
    id: impl Into<ElementId>,
    depth: usize,
    place: RowPlace,
    mark: RowMark,
    hovered_touching: Option<RowPlace>,
) -> Stateful<Div> {
    let is_current = place == RowPlace::Current;
    div()
        .id(id)
        .relative()
        .line_height(px(LINE_HEIGHT))
        .mx_2()
        .pl(indent(depth))
        .pr(px(ROW_INSET))
        .py(px(ROW_INSET))
        .flex()
        .items_start()
        .gap_2()
        .rounded(px(CONTROL_RADIUS))
        .border_1()
        .cursor_pointer()
        .text_color(match mark {
            _ if is_current => colors.text,
            RowMark::Read | RowMark::Gone => colors.muted_text,
            RowMark::Unread | RowMark::Page => colors.sidebar_text,
        })
        .map(|row| {
            if is_current {
                row.bg(colors.current)
                    .border_color(colors.current_border)
                    .font_weight(FontWeight::MEDIUM)
                    .map(|row| match hovered_touching {
                        Some(RowPlace::JustAboveCurrent) => row.rounded_t(px(0.)),
                        Some(RowPlace::JustBelowCurrent) => row.rounded_b(px(0.)),
                        _ => row,
                    })
            } else {
                row.border_color(gpui::transparent_black())
                    .map(|row| match place {
                        RowPlace::JustAboveCurrent => {
                            row.hover(|row| row.bg(colors.hover).rounded_b(px(0.)))
                        }
                        RowPlace::JustBelowCurrent => {
                            row.hover(|row| row.bg(colors.hover).rounded_t(px(0.)))
                        }
                        RowPlace::Current | RowPlace::Apart => {
                            row.hover(|row| row.bg(colors.hover))
                        }
                    })
                    .active(|row| row.bg(colors.pressed))
            }
        })
        .children((1..=depth).map(|level| guide_line(colors, level)))
        .child(mark_element(colors, mark, is_current))
}

/// A nested step hangs off a thin line under its parent's mark, one per level, so the tree reads
/// without leaning on indentation alone.
fn guide_line(colors: &Colors, level: usize) -> impl IntoElement {
    div()
        .absolute()
        .top(px(-1.))
        .bottom(px(-1.))
        .left(indent(level - 1) + px(MARK_SIZE / 2. - 0.5))
        .w(px(1.))
        .bg(colors.border)
}

/// A step that has something to check carries a small flag.
pub fn check_flag(colors: &Colors) -> impl IntoElement {
    div()
        .id("check-flag")
        .flex_none()
        .size(px(LINE_HEIGHT))
        .flex()
        .items_center()
        .justify_center()
        .tooltip(tooltip(|_| "Has something to check".into()))
        .child(sized_icon(Icon::Flag, colors.warning, px(12.)))
}

/// A row's label: it takes the room left and keeps clear of the row's right edge.
pub fn row_label(label: impl IntoElement) -> Div {
    div().flex_1().min_w_0().pr(px(TEXT_END)).child(label)
}

/// Small text after a row's label, centred on the label's first line like the badges.
pub fn row_note(colors: &Colors, note: impl Into<SharedString>) -> impl IntoElement {
    div()
        .flex_none()
        .mr(px(TEXT_END))
        .h(px(LINE_HEIGHT))
        .flex()
        .items_center()
        .text_xs()
        .text_color(colors.muted_text)
        .child(note.into())
}

/// A step the reviewer only needs to glance at.
pub fn skim_chip(colors: &Colors) -> impl IntoElement {
    div()
        .flex_none()
        .mr(px(PILL_END))
        .h(px(LINE_HEIGHT))
        .flex()
        .items_center()
        .child(chip(colors, Tone::Neutral).child("skim"))
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
            .min_w(px(LINE_HEIGHT))
            .h(px(LINE_HEIGHT))
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .px_1p5()
                    .rounded_full()
                    .bg(colors.surface)
                    .border_1()
                    .border_color(colors.subtle_border)
                    .text_xs()
                    .line_height(phi())
                    .text_color(colors.muted_text)
                    .child(count.to_string()),
            )
    })
}

fn indent(depth: usize) -> Pixels {
    px(FLAT_INDENT + INDENT_PER_DEPTH * depth as f32)
}

/// Where a row stands to the current one. Rows touch, so a hovered neighbour and the current row
/// square the corners where they meet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowPlace {
    Current,
    JustAboveCurrent,
    JustBelowCurrent,
    Apart,
}

/// The place of each row, from whether each is current, top to bottom.
pub fn row_places(is_current: &[bool]) -> Vec<RowPlace> {
    let is_current_at =
        |index: Option<usize>| index.and_then(|index| is_current.get(index)) == Some(&true);
    is_current
        .iter()
        .enumerate()
        .map(|(index, &is_current_row)| {
            if is_current_row {
                RowPlace::Current
            } else if is_current_at(Some(index + 1)) {
                RowPlace::JustAboveCurrent
            } else if is_current_at(index.checked_sub(1)) {
                RowPlace::JustBelowCurrent
            } else {
                RowPlace::Apart
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_rows_touching_the_current_one_know_their_side() {
        assert_eq!(
            row_places(&[false, false, true, false, false]),
            [
                RowPlace::Apart,
                RowPlace::JustAboveCurrent,
                RowPlace::Current,
                RowPlace::JustBelowCurrent,
                RowPlace::Apart,
            ]
        );
        assert_eq!(
            row_places(&[true, false]),
            [RowPlace::Current, RowPlace::JustBelowCurrent]
        );
        assert_eq!(
            row_places(&[false, false]),
            [RowPlace::Apart, RowPlace::Apart]
        );
    }
}
