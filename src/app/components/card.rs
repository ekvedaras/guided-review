use gpui::{Div, Styled, div, px};

use super::palette::{CARD_RADIUS, Colors};

/// A card on the page (a description, a thread, a comment box): the surface ground, a quiet edge
/// and, in the light, a hairline lift.
pub fn card(colors: &Colors) -> Div {
    div()
        .rounded(px(CARD_RADIUS))
        .border_1()
        .border_color(colors.subtle_border)
        .bg(colors.surface)
        .shadow(colors.card_shadow())
}
