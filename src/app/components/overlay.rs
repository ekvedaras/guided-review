use gpui::prelude::FluentBuilder;
use gpui::{
    Div, ElementId, InteractiveElement, ParentElement, Stateful, StatefulInteractiveElement,
    Styled, div, px,
};

use super::motion::{OVERLAY, STATE, fade_in, fade_out};
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
pub fn leaving_modal_layer(colors: &Colors, scrim_id: impl Into<ElementId>) -> Div {
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
            OVERLAY,
        ))
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
