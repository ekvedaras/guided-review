use std::time::{Duration, Instant};

use gpui::prelude::FluentBuilder;
use gpui::{
    AnimationElement, Bounds, Context, Deferred, Div, ElementId, InteractiveElement, IntoElement,
    ParentElement, Pixels, Point, Stateful, StatefulInteractiveElement, Styled, anchored, deferred,
    div, point, px,
};

use crate::viewer::{Closing, Motion, Presence, Shown};

use super::motion::{OVERLAY_RISE, STATE, along, fade_in, fade_out, panel_level, scrim_level};
use super::palette::{CARD_RADIUS, CONTROL_RADIUS, Colors, TEXT_FONT, inside_edge};
use super::sidebar_row::RowPlace;

/// The layer a modal overlay sits in: it covers the window, takes every click, and fades its
/// scrim in under the panel. The caller adds the close on a click outside and the panel.
pub fn modal_layer(colors: &Colors, scrim_id: impl Into<ElementId>) -> Div {
    div()
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .flex()
        .items_start()
        .justify_center()
        .occlude()
        .child(fade_in(
            div()
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .bg(colors.scrim),
            scrim_id,
            STATE,
        ))
}

/// A modal layer on its way out: its scrim fades away and the layer still takes every click, so
/// nothing under it reacts until it is gone.
pub fn leaving_modal_layer(
    colors: &Colors,
    scrim_id: impl Into<ElementId>,
    duration: Duration,
) -> Div {
    div()
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .occlude()
        .child(fade_out(
            div()
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .bg(colors.scrim),
            scrim_id,
            duration,
        ))
}

/// An overlay's scrim, fading in or out along the presence's timeline.
pub fn scrim_fade<E: IntoElement + Styled + 'static>(
    scrim: E,
    name: &'static str,
    presence: &Presence,
) -> AnimationElement<E> {
    along(
        scrim,
        (name, presence.turn()),
        presence.travel(),
        |scrim, position| scrim.opacity(scrim_level(position)),
    )
}

/// The layer of an overlay in whatever stage its `presence` is in. It takes every click while
/// entering and leaving, so nothing under the overlay reacts until it is gone. Name each overlay
/// once; the turn of the presence makes the ids of its moves new.
pub fn modal_layer_for(colors: &Colors, name: &'static str, presence: &Presence) -> Div {
    div()
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .flex()
        .items_start()
        .justify_center()
        .occlude()
        .child(scrim_fade(
            div()
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .bg(colors.scrim),
            name,
            presence,
        ))
}

/// An overlay's panel rising in, or sinking out while it leaves. A leaving panel is covered by a
/// layer that takes the mouse, so a button on it cannot be pressed on its way out.
pub fn rise_for<E: IntoElement + Styled + ParentElement + 'static>(
    panel: E,
    name: &'static str,
    presence: &Presence,
) -> AnimationElement<E> {
    let panel = match presence.shown() {
        Some(Shown::Leaving) => panel.child(input_blocker()),
        Some(Shown::Entering) | None => panel,
    };
    along(
        panel,
        (name, presence.turn()),
        presence.travel(),
        |panel, position| {
            let level = panel_level(position);
            panel
                .relative()
                .opacity(level)
                .top(OVERLAY_RISE * (1. - level))
        },
    )
}

/// A panel that floats over the page beside what opens it: a list, a card or a picker. It is
/// drawn on top, kept inside the window, and takes the mouse, so the page under it neither lights
/// up nor scrolls while the mouse is on it. The one way to build one: `gpui::deferred` is
/// disallowed elsewhere (`clippy.toml`), since a bare one lets hover and scroll through (#133).
#[expect(
    clippy::disallowed_methods,
    reason = "the one place a floating panel is built, so every one occludes"
)]
pub fn popover(panel: impl IntoElement) -> Deferred {
    deferred(
        anchored()
            .snap_to_window_with_margin(px(8.))
            .child(div().occlude().child(panel)),
    )
}

/// A `popover` whose top left corner is at `position` in the window, for a panel that belongs to
/// a spot inside a drawn text rather than to the element it is built in.
#[expect(
    clippy::disallowed_methods,
    reason = "a floating panel is built here, so it occludes like every other"
)]
pub fn popover_at(position: Point<Pixels>, panel: impl IntoElement) -> Deferred {
    deferred(
        anchored()
            .position(position)
            .snap_to_window_with_margin(px(8.))
            .child(div().occlude().child(panel)),
    )
}

/// Under `word`, or over it when the window has no room below; kept inside the window either way.
#[expect(
    clippy::disallowed_methods,
    reason = "a floating panel is built here, so it occludes like every other"
)]
pub fn popover_by(word: Bounds<Pixels>, panel: impl IntoElement) -> Deferred {
    // The window's edge flips the panel around `position`, without the offset: from under the
    // word's bottom to over its top.
    deferred(
        anchored()
            .position(word.origin)
            .offset(point(px(0.), word.size.height))
            .child(div().occlude().child(panel)),
    )
}

/// Covers what it is a child of and takes every mouse event that would reach it.
pub fn input_blocker() -> Div {
    div().absolute().top_0().left_0().size_full().occlude()
}

/// Closes the overlay whose presence `presence` picks: it leaves for `OVERLAY` with its move played
/// in reverse, or goes at once under reduced motion. Whoever owns the overlay has its `close`
/// refuse unless `Presence::is_open`, so a click or key during the leave reaches nothing.
pub fn close_presence<T: 'static>(
    this: &mut T,
    cx: &mut Context<T>,
    presence: fn(&mut T) -> &mut Presence,
) {
    let motion = if cx.reduce_motion() {
        Motion::Reduced
    } else {
        Motion::Full
    };
    if let Closing::Leaving { ticket, lasts } = presence(this).close(motion, Instant::now()) {
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(lasts).await;
            this.update(cx, |this, cx| {
                presence(this).end_leaving(ticket);
                cx.notify();
            })
        })
        .detach();
    }
}

/// What floats over the page: the overlay ground, its edge and its two-layer shadow.
pub fn overlay_panel(colors: &Colors) -> Div {
    div()
        .flex()
        .flex_col()
        .overflow_hidden()
        .bg(colors.overlay)
        .border_1()
        .border_color(colors.border)
        .rounded(px(CARD_RADIUS))
        .shadow(colors.overlay_shadow())
        .font_family(*TEXT_FONT)
        .text_sm()
        .text_color(colors.text)
}

/// Where a picker row sits against the selected row and the hovered one, from the rows directly
/// above and below it (`None` at a heading, an unfolded body or an edge: nothing touches there).
/// Every list of rows that touch builds its rows from this and `picker_row_at`, so the corners they
/// share are squared in one way.
pub fn touching_places<T: PartialEq + Copy>(
    row: T,
    above: Option<T>,
    below: Option<T>,
    selected: T,
    hovered: Option<T>,
) -> (RowPlace, Option<RowPlace>) {
    let place = if row == selected {
        RowPlace::Current
    } else if below == Some(selected) {
        RowPlace::JustAboveCurrent
    } else if above == Some(selected) {
        RowPlace::JustBelowCurrent
    } else {
        RowPlace::Apart
    };
    let hovered_touching = match hovered {
        Some(hovered) if row == selected && hovered != selected && above == Some(hovered) => {
            Some(RowPlace::JustAboveCurrent)
        }
        Some(hovered) if row == selected && hovered != selected && below == Some(hovered) => {
            Some(RowPlace::JustBelowCurrent)
        }
        _ => None,
    };
    (place, hovered_touching)
}

/// A picker row that touches its neighbours: the selected row and a hovered neighbour square the
/// corners where they meet, as sidebar rows do.
pub fn picker_row_at(
    colors: &Colors,
    id: impl Into<ElementId>,
    place: RowPlace,
    hovered_touching: Option<RowPlace>,
) -> Stateful<Div> {
    div()
        .id(id)
        .mx_2()
        .px_2()
        .flex()
        .items_center()
        .gap_3()
        .rounded(px(CONTROL_RADIUS))
        .border_1()
        .overflow_hidden()
        .cursor_pointer()
        .map(|row| match place {
            RowPlace::Current => row
                .bg(colors.current)
                .border_color(colors.current_border)
                .map(|row| match hovered_touching {
                    Some(RowPlace::JustAboveCurrent) => row.rounded_t(px(0.)),
                    Some(RowPlace::JustBelowCurrent) => row.rounded_b(px(0.)),
                    _ => row,
                }),
            RowPlace::JustAboveCurrent => row
                .border_color(gpui::transparent_black())
                .hover(|row| row.bg(colors.hover).rounded_b(px(0.))),
            RowPlace::JustBelowCurrent => row
                .border_color(gpui::transparent_black())
                .hover(|row| row.bg(colors.hover).rounded_t(px(0.))),
            RowPlace::Apart => row
                .border_color(gpui::transparent_black())
                .hover(|row| row.bg(colors.hover)),
        })
        .active(|row| row.bg(colors.pressed))
}

/// The strip under a picker's rows that names its keys, at the foot of an `overlay_panel`.
pub fn picker_footer(colors: &Colors) -> Div {
    div()
        .rounded_b(px(inside_edge(CARD_RADIUS)))
        .px_4()
        .py_2()
        .flex()
        .items_center()
        .gap_4()
        .border_t_1()
        .border_color(colors.subtle_border)
        .bg(colors.header_band())
        .text_xs()
        .text_color(colors.muted_text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_selected_row_squares_the_side_a_hovered_neighbour_touches() {
        assert_eq!(
            touching_places(2, Some(1), Some(3), 2, Some(3)),
            (RowPlace::Current, Some(RowPlace::JustBelowCurrent))
        );
        assert_eq!(
            touching_places(2, Some(1), Some(3), 2, Some(1)),
            (RowPlace::Current, Some(RowPlace::JustAboveCurrent))
        );
    }

    #[test]
    fn a_neighbour_of_the_selected_row_knows_which_side_touches_it() {
        assert_eq!(
            touching_places(1, Some(0), Some(2), 2, None),
            (RowPlace::JustAboveCurrent, None)
        );
        assert_eq!(
            touching_places(3, Some(2), None, 2, None),
            (RowPlace::JustBelowCurrent, None)
        );
    }

    #[test]
    fn rows_apart_from_the_selected_one_square_nothing() {
        assert_eq!(
            touching_places(0, None, Some(1), 3, Some(1)),
            (RowPlace::Apart, None)
        );
        assert_eq!(
            touching_places(2, None, None, 2, Some(3)),
            (RowPlace::Current, None)
        );
    }
}
