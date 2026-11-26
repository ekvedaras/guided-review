//! Text the reader can drag over and copy with the prose selection (`ProseSelections`): plain
//! words of their own, and the drag handling a markdown body's texts share.

use gpui::{
    AnyElement, App, InteractiveElement, IntoElement, MouseButton, ParentElement, RenderOnce,
    SharedString, Styled, StyledText, TextLayout, Window, div,
};

use crate::app::code_selection::TextSpot;
use crate::app::prose_selection::ProseSelections;

use super::diff_view::{byte_at, with_selection};
use super::palette::Colors;

/// Plain words the reader can drag over and copy, as a body of their own under the key: a
/// problem, a toast, a command to run. They take their size and colour from the element around
/// them. Not for text in a row that acts on a click, where a press must not start a selection.
#[derive(IntoElement)]
pub struct SelectableText {
    key: SharedString,
    text: SharedString,
}

impl SelectableText {
    pub fn new(key: impl Into<SharedString>, text: impl Into<SharedString>) -> Self {
        Self {
            key: key.into(),
            text: text.into(),
        }
    }
}

impl RenderOnce for SelectableText {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        ProseSelections::body_drawn(&self.key, cx);
        let row = ProseSelections::text_drawn(&self.key, self.text.clone(), cx);
        let highlights = ProseSelections::range_on(&self.key, row, self.text.len(), cx)
            .map_or_else(Vec::new, |selected| {
                with_selection(&[], selected, Colors::active(cx).text_selection)
            });
        let styled = StyledText::new(self.text).with_highlights(highlights);
        let layout = styled.layout().clone();
        selectable_text(styled.into_any_element(), Some((self.key, row)), layout)
    }
}

/// With `row`, the text is that row of the body's prose selection: a press starts a selection on
/// it, a drag carries it on.
pub(super) fn selectable_text(
    text: AnyElement,
    row: Option<(SharedString, usize)>,
    layout: TextLayout,
) -> AnyElement {
    let Some((body, row)) = row else {
        return text;
    };
    let (press_body, move_body) = (body.clone(), body);
    let (press_layout, move_layout) = (layout.clone(), layout);
    div()
        .cursor_text()
        .on_mouse_down(MouseButton::Left, move |event, window, cx| {
            let at = TextSpot {
                row,
                byte: byte_at(&press_layout, event.position),
            };
            match event.click_count {
                2 => ProseSelections::press_twice(&press_body, at, cx),
                _ => ProseSelections::press(press_body.clone(), at, cx),
            }
            window.refresh();
        })
        .on_mouse_move(move |event, window, cx| {
            if event.pressed_button == Some(MouseButton::Left) {
                let at = TextSpot {
                    row,
                    byte: byte_at(&move_layout, event.position),
                };
                ProseSelections::drag_over(&move_body, at, cx);
                window.refresh();
            }
        })
        .on_mouse_up(MouseButton::Left, |_, window, cx| {
            ProseSelections::end_drag(cx);
            window.refresh();
        })
        .on_mouse_up_out(MouseButton::Left, |_, window, cx| {
            ProseSelections::end_drag(cx);
            window.refresh();
        })
        .child(text)
        .into_any_element()
}
