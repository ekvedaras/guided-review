use gpui::prelude::FluentBuilder;
use gpui::{
    App, Div, ElementId, FontWeight, InteractiveElement, IntoElement, ParentElement, Pixels, Rgba,
    Stateful, StatefulInteractiveElement, Styled, Svg, div, px,
};

use super::icon::{Icon, sized_icon};
use super::key_cap::{CapGround, chord_caps};
use super::palette::{CONTROL_RADIUS, Colors};
use super::tooltip::tooltip;
use crate::app::keymap::keymap;
use crate::viewer::Command;

/// How loud a button is. A screen has one `Primary`: the step the reader takes next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonKind {
    Primary,
    /// A raised key: the ordinary action of a form or a card.
    Secondary,
    /// The accent action beside content that is not the screen's way on, such as Ask.
    Tinted,
    /// For actions next to content, which should not pull the eye: open on GitHub, fold, cancel.
    Ghost,
    /// A toggle whose option is on.
    Pressed,
    /// Something the reader finished, like a step marked as read; a click undoes it.
    Done,
    /// Takes something away: remove a clone, discard a draft.
    Danger,
    /// One part of a `joined` group, which draws the edge and the ground for all its parts.
    Joined,
    /// A part of a `joined` group whose option is on.
    JoinedOn,
}

impl ButtonKind {
    fn is_joined(self) -> bool {
        matches!(self, Self::Joined | Self::JoinedOn)
    }

    /// An icon's colour at rest, and under the mouse: it follows the label.
    fn icon_colors(self, colors: &Colors) -> (Rgba, Rgba) {
        match self {
            Self::Primary => (colors.on_accent_fill, colors.on_accent_fill),
            Self::Secondary | Self::Joined => (colors.muted_text, colors.accent),
            Self::Tinted | Self::Pressed | Self::JoinedOn => (colors.accent, colors.accent),
            Self::Ghost => (colors.muted_text, colors.text),
            Self::Done => (colors.success, colors.success),
            Self::Danger => (colors.danger, colors.danger),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonSize {
    /// The way on of a screen or a form, at its foot.
    Large,
    Regular,
    /// Inside a header band, a card or next to a line.
    Small,
}

impl ButtonSize {
    pub fn height(self) -> Pixels {
        match self {
            Self::Large => px(40.),
            Self::Regular => px(32.),
            Self::Small => px(26.),
        }
    }

    fn padding(self) -> Pixels {
        match self {
            Self::Large => px(20.),
            Self::Regular => px(14.),
            Self::Small => px(10.),
        }
    }

    fn gap(self) -> Pixels {
        match self {
            Self::Large => px(8.),
            Self::Regular | Self::Small => px(6.),
        }
    }

    pub fn icon_size(self) -> Pixels {
        match self {
            Self::Large => px(16.),
            Self::Regular => px(14.),
            Self::Small => px(13.),
        }
    }
}

/// Buttons name their group, so a drawn icon inside can follow the button's hover.
const BUTTON_GROUP: &str = "button";

/// The look every button of the app shares, with its hover and pressed states; the caller adds the
/// label and the click.
pub fn button(
    colors: &Colors,
    id: impl Into<ElementId>,
    kind: ButtonKind,
    size: ButtonSize,
) -> Stateful<Div> {
    let height = if kind.is_joined() {
        size.height() - px(2.)
    } else {
        size.height()
    };
    let base = div()
        .id(id)
        .group(BUTTON_GROUP)
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .gap(size.gap())
        .h(height)
        .px(size.padding())
        .whitespace_nowrap()
        .cursor_pointer()
        .font_weight(FontWeight::MEDIUM)
        .map(|button| match size {
            ButtonSize::Large | ButtonSize::Regular => button.text_sm(),
            ButtonSize::Small => button.text_xs(),
        })
        .when(!kind.is_joined(), |button| {
            button.rounded(px(CONTROL_RADIUS)).border_1()
        });
    match kind {
        ButtonKind::Primary => base
            .bg(colors.primary_fill())
            .border_color(colors.accent_fill.end)
            .shadow(colors.primary_shadow())
            .text_color(colors.on_accent_fill)
            .hover(|button| {
                button
                    .bg(colors.primary_fill_hover())
                    .border_color(colors.accent_fill_hover.end)
            })
            .active(|button| {
                button
                    .bg(colors.accent_fill_pressed)
                    .border_color(colors.accent_fill_pressed)
                    .shadow_none()
            }),
        ButtonKind::Secondary => base
            .bg(colors.raised_fill())
            .border_color(colors.border)
            .shadow(colors.control_shadow())
            .text_color(colors.text)
            .hover(|button| {
                button
                    .border_color(colors.accent_border)
                    .text_color(colors.accent)
            })
            .active(|button| {
                button
                    .bg(colors.pressed)
                    .border_color(colors.border)
                    .shadow_none()
            }),
        ButtonKind::Tinted => base
            .bg(colors.raised_fill())
            .border_color(colors.current_border)
            .shadow(colors.control_shadow())
            .text_color(colors.accent)
            .hover(|button| button.bg(colors.current).border_color(colors.accent_border))
            .active(|button| button.bg(colors.pressed).shadow_none()),
        ButtonKind::Ghost => base
            .border_color(gpui::transparent_black())
            .text_color(colors.muted_text)
            .hover(|button| button.bg(colors.hover).text_color(colors.text))
            .active(|button| button.bg(colors.pressed).text_color(colors.text)),
        ButtonKind::Pressed => base
            .bg(colors.current)
            .border_color(colors.current_border)
            .text_color(colors.accent)
            .hover(|button| button.border_color(colors.accent_border))
            .active(|button| button.bg(colors.pressed)),
        ButtonKind::Done => base
            .bg(colors.success_fill.top_down())
            .border_color(colors.success_border)
            .shadow(colors.control_shadow())
            .text_color(colors.success)
            .hover(|button| button.border_color(colors.success))
            .active(|button| button.bg(colors.success_background).shadow_none()),
        ButtonKind::Danger => base
            .border_color(gpui::transparent_black())
            .text_color(colors.danger)
            .hover(|button| {
                button
                    .bg(colors.danger_background)
                    .border_color(colors.danger_border)
            })
            .active(|button| {
                button
                    .bg(colors.danger_background)
                    .border_color(colors.danger)
            }),
        ButtonKind::Joined => base
            .text_color(colors.text)
            .hover(|button| button.bg(colors.hover).text_color(colors.accent))
            .active(|button| button.bg(colors.pressed)),
        ButtonKind::JoinedOn => base
            .bg(colors.current)
            .text_color(colors.accent)
            .active(|button| button.bg(colors.pressed)),
    }
}

/// A drawn icon in a button, in the colour of its label, following the button's hover.
pub fn button_icon(colors: &Colors, kind: ButtonKind, size: ButtonSize, icon: Icon) -> Svg {
    let (at_rest, under_mouse) = kind.icon_colors(colors);
    sized_icon(icon, at_rest, size.icon_size())
        .group_hover(BUTTON_GROUP, move |icon| icon.text_color(under_mouse))
}

/// A button that leads with an icon of what it does.
pub fn icon_label_button(
    colors: &Colors,
    id: impl Into<ElementId>,
    kind: ButtonKind,
    size: ButtonSize,
    icon: Icon,
    label: impl IntoElement,
) -> Stateful<Div> {
    button(colors, id, kind, size)
        .child(button_icon(colors, kind, size, icon))
        .child(label)
}

/// A square button that is only an icon; the tooltip names what it does.
pub fn icon_button(
    colors: &Colors,
    id: impl Into<ElementId>,
    kind: ButtonKind,
    size: ButtonSize,
    icon: Icon,
    name: &'static str,
) -> Stateful<Div> {
    button(colors, id, kind, size)
        .w(size.height())
        .px_0()
        .child(button_icon(colors, kind, size, icon))
        .tooltip(tooltip(move |_| name.into()))
}

/// Buttons that belong together drawn as one raised control, split by hairlines: the toggles of
/// a toolbar, or one action with a variant ("Ask agent" and "now"). Each part is a `Joined` or
/// `JoinedOn` button.
pub fn joined(colors: &Colors, parts: Vec<Stateful<Div>>) -> Div {
    let last = parts.len().saturating_sub(1);
    let inner = px(CONTROL_RADIUS - 1.);
    div()
        .flex()
        .flex_none()
        .items_center()
        .rounded(px(CONTROL_RADIUS))
        .border_1()
        .border_color(colors.border)
        .bg(colors.raised_fill())
        .shadow(colors.control_shadow())
        .children(parts.into_iter().enumerate().map(move |(index, part)| {
            part.when(index == 0, |part| part.rounded_l(inner))
                .when(index == last, |part| part.rounded_r(inner))
                .when(index > 0, |part| {
                    part.border_l_1().border_color(colors.border)
                })
        }))
}

/// A button that stays pressed while its option is on.
pub fn toggle_button(
    colors: &Colors,
    id: impl Into<ElementId>,
    is_on: bool,
    size: ButtonSize,
) -> Stateful<Div> {
    let kind = if is_on {
        ButtonKind::Pressed
    } else {
        ButtonKind::Secondary
    };
    button(colors, id, kind, size)
}

/// The keys that do what the button does, at its trailing end, on the button's own ground.
pub fn button_keys(colors: &Colors, kind: ButtonKind, command: Command, cx: &App) -> Div {
    let caps = keymap(cx)
        .first_chord(command)
        .map(|chord| chord_caps(colors, chord, CapGround::of(kind)));
    div().flex_none().ml_1().children(caps)
}
