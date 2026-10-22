use gpui::{
    Context, EventEmitter, FocusHandle, InteractiveElement, IntoElement, KeyDownEvent,
    ParentElement, Render, SharedString, StatefulInteractiveElement, Styled, Window, div, px,
};

use crate::viewer::{JumpKind, JumpTarget};

use super::palette::{Palette, TEXT_FONT};
use super::text_input::TextDraft;

const VISIBLE_ROWS: usize = 12;

pub enum JumpEvent {
    /// The list must be asked again for this text.
    QueryChanged(String),
    Picked(JumpTarget),
}

/// The cmd+k overlay: a query box over the places the reviewer can jump to.
/// It knows nothing of the review; its owner answers `QueryChanged` with new targets.
pub struct JumpPalette {
    is_open: bool,
    draft: TextDraft,
    targets: Vec<JumpTarget>,
    selected: usize,
    focus: FocusHandle,
    owner_focus: FocusHandle,
}

impl EventEmitter<JumpEvent> for JumpPalette {}

impl JumpPalette {
    pub const KEY_CONTEXT: &str = "JumpInput";

    pub fn new(owner_focus: FocusHandle, cx: &mut Context<Self>) -> Self {
        Self {
            is_open: false,
            draft: TextDraft::default(),
            targets: Vec::new(),
            selected: 0,
            focus: cx.focus_handle(),
            owner_focus,
        }
    }

    pub fn is_open(&self) -> bool {
        self.is_open
    }

    pub fn open(&mut self, targets: Vec<JumpTarget>, window: &mut Window, cx: &mut Context<Self>) {
        self.is_open = true;
        self.draft.clear();
        self.set_targets(targets);
        window.focus(&self.focus, cx);
        cx.notify();
    }

    pub fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.is_open = false;
        window.focus(&self.owner_focus, cx);
        cx.notify();
    }

    pub fn set_targets(&mut self, targets: Vec<JumpTarget>) {
        self.targets = targets;
        self.selected = 0;
    }

    fn pick(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(target) = self.targets.get(self.selected).cloned() {
            self.close(window, cx);
            cx.emit(JumpEvent::Picked(target));
        }
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        match keystroke.key.as_str() {
            "enter" => self.pick(window, cx),
            "escape" => self.close(window, cx),
            "down" => self.selected = (self.selected + 1).min(self.targets.len().saturating_sub(1)),
            "up" => self.selected = self.selected.saturating_sub(1),
            _ => {
                if self.draft.edit(keystroke) {
                    cx.emit(JumpEvent::QueryChanged(self.draft.text().to_string()));
                }
            }
        }
        cx.notify();
    }

    fn row(&self, index: usize, target: &JumpTarget, cx: &mut Context<Self>) -> impl IntoElement {
        let kind = match target.kind {
            JumpKind::Step => "step",
            JumpKind::File => "file",
            JumpKind::Symbol => "symbol",
        };
        let row = div()
            .id(SharedString::from(format!("jump-{index}")))
            .flex()
            .gap_2()
            .px_3()
            .py_1()
            .cursor_pointer()
            .on_click(cx.listener(move |this, _, window, cx| {
                this.selected = index;
                this.pick(window, cx);
            }))
            .child(
                div()
                    .w(px(48.))
                    .text_color(Palette::muted_text())
                    .child(kind),
            )
            .child(div().flex_1().child(target.label.clone()))
            .child(
                div()
                    .text_color(Palette::muted_text())
                    .child(target.detail.clone()),
            );
        if index == self.selected {
            row.bg(Palette::current())
        } else {
            row
        }
    }
}

impl Render for JumpPalette {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.is_open {
            return div().into_any_element();
        }
        let first_shown = self.selected.saturating_sub(VISIBLE_ROWS - 1);
        let rows: Vec<_> = self
            .targets
            .iter()
            .enumerate()
            .skip(first_shown)
            .take(VISIBLE_ROWS)
            .map(|(index, target)| self.row(index, target, cx).into_any_element())
            .collect();
        div()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .flex()
            .justify_center()
            .child(
                div()
                    .id("jump-palette")
                    .key_context(Self::KEY_CONTEXT)
                    .track_focus(&self.focus)
                    .on_key_down(cx.listener(Self::on_key_down))
                    .mt(px(80.))
                    .w(px(640.))
                    .h_auto()
                    .max_h(px(480.))
                    .flex()
                    .flex_col()
                    .bg(Palette::background())
                    .border_1()
                    .border_color(Palette::border())
                    .rounded_md()
                    .shadow_lg()
                    .font_family(TEXT_FONT)
                    .text_sm()
                    .text_color(Palette::text())
                    .child(
                        div()
                            .p_3()
                            .border_b_1()
                            .border_color(Palette::border())
                            .child(self.draft.shown()),
                    )
                    .children(rows),
            )
            .into_any_element()
    }
}
