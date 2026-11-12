use std::time::Duration;

use gpui::prelude::FluentBuilder;
use gpui::{
    AnimationElement, Context, Div, ElementId, InteractiveElement, IntoElement, ParentElement,
    Stateful, StatefulInteractiveElement, Styled, div, px,
};

use crate::viewer::{Closing, Motion, Presence, Shown};

use super::motion::{OVERLAY, STATE, fade_in, fade_out, rise_in, sink_out};
use super::palette::{CARD_RADIUS, CONTROL_RADIUS, Colors, TEXT_FONT, inside_edge};

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

/// The layer of an overlay in whatever stage its `presence` is in: entering, or leaving with its
/// scrim fading out. Both stages take every click, so nothing under the overlay reacts until it is
/// gone. Name each overlay once; the turn of the presence makes the ids of its moves new.
pub fn modal_layer_for(colors: &Colors, name: &'static str, presence: &Presence) -> Div {
    let id = (name, presence.turn());
    match presence.shown() {
        Some(Shown::Leaving) => leaving_modal_layer(colors, id, STATE),
        Some(Shown::Entering) | None => modal_layer(colors, id),
    }
}

/// An overlay's panel rising in, or sinking out while it leaves. A leaving panel is covered by a
/// layer that takes the mouse, so a button on it cannot be pressed on its way out.
pub fn rise_for<E: IntoElement + Styled + ParentElement + 'static>(
    panel: E,
    name: &'static str,
    presence: &Presence,
) -> AnimationElement<E> {
    let id = (name, presence.turn());
    match presence.shown() {
        Some(Shown::Leaving) => sink_out(panel.child(input_blocker()), id),
        Some(Shown::Entering) | None => rise_in(panel, id),
    }
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
    if let Closing::Leaving(ticket) = presence(this).close(motion) {
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(OVERLAY).await;
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
        .font_family(TEXT_FONT)
        .text_sm()
        .text_color(colors.text)
}

/// A row of a picker list (jump palette, PR switcher): the selected one sits on `current` with
/// its edge, the others light up under the mouse, and every one darkens while pressed.
pub fn picker_row(colors: &Colors, id: impl Into<ElementId>, is_selected: bool) -> Stateful<Div> {
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
        .map(|row| {
            if is_selected {
                row.bg(colors.current).border_color(colors.current_border)
            } else {
                row.border_color(gpui::transparent_black())
                    .hover(|row| row.bg(colors.hover))
            }
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
