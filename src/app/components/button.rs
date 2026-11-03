use gpui::prelude::FluentBuilder;
use gpui::{Div, ElementId, FontWeight, InteractiveElement, Pixels, Stateful, Styled, div, px};

use super::palette::Colors;

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
    Regular,
    /// Inside a header band or next to a line.
    Small,
}

impl ButtonSize {
    fn height(self) -> Pixels {
        match self {
            Self::Regular => px(28.),
            Self::Small => px(22.),
        }
    }
}

/// The look every button of the app shares; the caller adds the label and the click.
pub fn button(
    colors: Colors,
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
            ButtonSize::Regular => px(12.),
            ButtonSize::Small => px(8.),
        })
        .rounded_md()
        .border_1()
        .whitespace_nowrap()
        .cursor_pointer()
        .map(|button| match size {
            ButtonSize::Regular => button.text_sm(),
            ButtonSize::Small => button.text_xs(),
        });
    match kind {
        ButtonKind::Primary => base
            .bg(colors.accent_fill)
            .border_color(colors.accent_fill)
            .text_color(colors.on_accent_fill)
            .font_weight(FontWeight::MEDIUM)
            .hover(|button| {
                button
                    .bg(colors.accent_fill_hover)
                    .border_color(colors.accent_fill_hover)
            }),
        ButtonKind::Secondary => base
            .bg(colors.surface)
            .border_color(colors.border)
            .text_color(colors.text)
            .hover(|button| button.bg(colors.hover)),
        ButtonKind::Ghost => base
            .border_color(gpui::transparent_black())
            .text_color(colors.muted_text)
            .hover(|button| button.bg(colors.hover).text_color(colors.text)),
        ButtonKind::Pressed => base
            .bg(colors.current)
            .border_color(colors.accent)
            .text_color(colors.accent),
        ButtonKind::Done => base
            .bg(colors.success_background)
            .border_color(colors.success)
            .text_color(colors.success)
            .hover(|button| button.bg(colors.hover)),
    }
}

/// A button that stays pressed while its option is on.
pub fn toggle_button(
    colors: Colors,
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
