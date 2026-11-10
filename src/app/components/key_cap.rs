use gpui::{IntoElement, ParentElement, Styled, div, px};

use super::icon::{Icon, sized_icon};
use super::palette::{CONTROL_RADIUS, Colors};

/// What a key cap shows: the key's name, or a drawn arrow for the keys that have no word.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyLabel {
    Text(&'static str),
    Icon(Icon),
}

pub fn key_cap(colors: &Colors, key: KeyLabel) -> impl IntoElement {
    let cap = div()
        .min_w(px(18.))
        .h(px(18.))
        .px_1()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .rounded(px(CONTROL_RADIUS))
        .border_1()
        .border_color(colors.border)
        .bg(colors.header_band())
        .shadow(colors.card_shadow())
        .text_color(colors.text);
    match key {
        KeyLabel::Text(name) => cap.child(name),
        KeyLabel::Icon(icon) => cap.child(sized_icon(icon, colors.text, px(11.))),
    }
}

/// Keys, then what they do.
pub fn key_hint(colors: &Colors, keys: &[KeyLabel], what: &'static str) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .gap_1()
        .children(keys.iter().map(|key| key_cap(colors, *key)))
        .child(div().pl_1().child(what))
}
