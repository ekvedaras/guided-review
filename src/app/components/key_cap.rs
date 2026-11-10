use gpui::{Div, IntoElement, ParentElement, Styled, div, px};

use crate::viewer::{Chord, KeyCap};

use super::button::ButtonKind;
use super::icon::{Icon, sized_icon};
use super::palette::{Colors, INNER_RADIUS};

/// What a key cap shows: the key's name, or a drawn arrow for the keys that have no word.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyLabel {
    Text(&'static str),
    Icon(Icon),
}

impl KeyLabel {
    pub fn of(cap: KeyCap) -> Self {
        match cap {
            KeyCap::Cmd => Self::Text("⌘"),
            KeyCap::Shift => Self::Text("⇧"),
            KeyCap::Left => Self::Icon(Icon::ArrowLeft),
            KeyCap::Right => Self::Icon(Icon::ArrowRight),
            KeyCap::Up => Self::Icon(Icon::ArrowUp),
            KeyCap::Down => Self::Icon(Icon::ArrowDown),
            KeyCap::Enter => Self::Icon(Icon::Enter),
            KeyCap::Escape => Self::Text("esc"),
            KeyCap::Key { face, .. } => Self::Text(face),
        }
    }
}

/// What a cap stands on: the page or a raised control, or a primary button's accent fill.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapGround {
    Page,
    Accent,
}

impl CapGround {
    /// A cap inside a button stands on that button's ground.
    pub fn of(kind: ButtonKind) -> Self {
        match kind {
            ButtonKind::Primary => Self::Accent,
            _ => Self::Page,
        }
    }
}

pub fn key_cap(colors: &Colors, key: KeyLabel) -> impl IntoElement {
    key_cap_on(colors, key, CapGround::Page)
}

pub fn key_cap_on(colors: &Colors, key: KeyLabel, ground: CapGround) -> impl IntoElement {
    let cap = div()
        .min_w(px(18.))
        .h(px(18.))
        .px_1()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .rounded(px(INNER_RADIUS))
        .border_1()
        .text_xs();
    let (cap, face) = match ground {
        CapGround::Page => (
            cap.border_color(colors.border)
                .bg(colors.header_band())
                .shadow(colors.card_shadow())
                .text_color(colors.text),
            colors.text,
        ),
        CapGround::Accent => (
            cap.border_color(colors.on_accent_fill.opacity(0.35))
                .bg(colors.on_accent_fill.opacity(0.16))
                .text_color(colors.on_accent_fill),
            colors.on_accent_fill,
        ),
    };
    match key {
        KeyLabel::Text(name) => cap.child(name),
        KeyLabel::Icon(icon) => cap.child(sized_icon(icon, face, px(11.))),
    }
}

/// The caps of one chord, side by side.
pub fn chord_caps(colors: &Colors, chord: Chord, ground: CapGround) -> Div {
    div().flex().flex_none().items_center().gap_0p5().children(
        chord
            .iter()
            .map(|cap| key_cap_on(colors, KeyLabel::of(*cap), ground)),
    )
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
