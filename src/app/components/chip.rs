use crate::app::updater::running_build;
use gpui::{AnimationElement, Div, ElementId, ParentElement, Styled, div, phi, px};

use super::motion::{BANNER_DROP, Edge, STATE, slide_in};
use super::palette::{CARD_RADIUS, Colors, INNER_RADIUS, Tone};

/// A small mark of a state beside a name: "Resolved", "draft", "changed since your read".
pub fn chip(colors: &Colors, tone: Tone) -> Div {
    let tone = colors.tone(tone);
    div()
        .flex_none()
        .flex()
        .items_center()
        .gap_1()
        .px_1p5()
        .rounded(px(INNER_RADIUS))
        .border_1()
        .border_color(tone.border)
        .bg(tone.background)
        .text_color(tone.text)
        .text_xs()
        .line_height(phi())
}

/// Marks a window not of the release install, `DEV` or `TEST`, so it is not taken for the app that updates.
pub fn build_chip(colors: &Colors) -> Option<Div> {
    running_build().badge().map(|badge| {
        chip(colors, Tone::Warning)
            .font_weight(gpui::FontWeight::BOLD)
            .child(badge)
    })
}

/// A note across the page column under the title bar, in its state's ground with an edge below.
pub fn notice_band(colors: &Colors, tone: Tone) -> Div {
    let tone = colors.tone(tone);
    div()
        .flex_none()
        .px_6()
        .py_1p5()
        .flex()
        .items_center()
        .gap_3()
        .bg(tone.background)
        .border_b_1()
        .border_color(tone.border)
        .text_sm()
        .text_color(tone.text)
}

/// A band arrives by dropping in from the title bar.
pub fn dropping_in(band: Div, id: impl Into<ElementId>) -> AnimationElement<Div> {
    slide_in(band, id, STATE, Edge::Top, BANNER_DROP)
}

/// A note inside the page, in its state's ground with an edge all round.
pub fn callout(colors: &Colors, tone: Tone) -> Div {
    let tone = colors.tone(tone);
    div()
        .px_4()
        .py_3()
        .rounded(px(CARD_RADIUS))
        .border_1()
        .border_color(tone.border)
        .bg(tone.background)
}
