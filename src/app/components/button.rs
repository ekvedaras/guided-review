use gpui::prelude::FluentBuilder;
use gpui::{
    Div, ElementId, FontWeight, InteractiveElement, Pixels, Stateful, StatefulInteractiveElement,
    Styled, div, px,
};

use super::palette::{CONTROL_RADIUS, Colors};

/// How loud a button is. A screen has one `Primary`: the step the reader takes next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonKind {
    Primary,
    Secondary,
    /// For actions next to content, which should not pull the eye: ask, fold, more lines.
    Ghost,
    /// A toggle whose option is on.
    Pressed,
    /// Something the reader finished, like a step marked as read; a click undoes it.
    Done,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonSize {
    /// The screen's way on, at the foot of the page.
    Large,
    Regular,
    /// Inside a header band or next to a line.
    Small,
}

impl ButtonSize {
    fn height(self) -> Pixels {
        match self {
            Self::Large => px(36.),
            Self::Regular => px(28.),
            Self::Small => px(22.),
        }
    }
}

/// The look every button of the app shares, with its hover and pressed states; the caller adds the
/// label and the click.
pub fn button(
    colors: &Colors,
    id: impl Into<ElementId>,
    kind: ButtonKind,
    size: ButtonSize,
) -> Stateful<Div> {
    let base = div()
        .id(id)
        .flex()
        .flex_none()
        .items_center()
        .gap_1()
        .h(size.height())
        .px(match size {
            ButtonSize::Large => px(16.),
            ButtonSize::Regular => px(12.),
            ButtonSize::Small => px(8.),
        })
        .rounded(px(CONTROL_RADIUS))
        .border_1()
        .whitespace_nowrap()
        .cursor_pointer()
        .map(|button| match size {
            ButtonSize::Large | ButtonSize::Regular => button.text_sm(),
            ButtonSize::Small => button.text_xs(),
        });
    match kind {
        ButtonKind::Primary => base
            .bg(colors.primary_fill())
            .border_color(colors.accent_fill.end)
            .shadow(colors.primary_shadow())
            .text_color(colors.on_accent_fill)
            .font_weight(FontWeight::MEDIUM)
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
            .bg(colors.surface)
            .border_color(colors.border)
            .shadow(colors.card_shadow())
            .text_color(colors.text)
            .hover(|button| button.bg(colors.hover))
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
            .hover(|button| button.border_color(colors.accent))
            .active(|button| button.bg(colors.pressed)),
        ButtonKind::Done => base
            .bg(colors.success_background)
            .border_color(colors.success_border)
            .text_color(colors.success)
            .hover(|button| button.border_color(colors.success))
            .active(|button| button.bg(colors.pressed)),
    }
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
