use gpui::{
    Context, EventEmitter, FocusHandle, InteractiveElement, IntoElement, KeyDownEvent,
    ParentElement, Render, SharedString, StatefulInteractiveElement, Styled, Window, div, px,
};

use crate::viewer::{JumpKind, JumpTarget};

use super::palette::{Colors, TEXT_FONT};
use super::text_box::TextBox;
use super::text_input::Lines;

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
    query: String,
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
            query: String::new(),
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
        self.query.clear();
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

    /// Up and down pick a target; the query box takes every other key.
    fn on_key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        match event.keystroke.key.as_str() {
            "down" => self.selected = (self.selected + 1).min(self.targets.len().saturating_sub(1)),
            "up" => self.selected = self.selected.saturating_sub(1),
            _ => return,
        }
        cx.notify();
    }

    fn query_box(&self, cx: &mut Context<Self>) -> TextBox {
        let (on_edit, on_submit, on_cancel) = (cx.entity(), cx.entity(), cx.entity());
        TextBox::new("jump-query", &self.focus, &self.query, Lines::Single)
            .on_edit(move |query, _, cx| {
                on_edit.update(cx, |this, cx| {
                    this.query = query.clone();
                    cx.emit(JumpEvent::QueryChanged(query));
                    cx.notify();
                })
            })
            .on_submit(move |window, cx| on_submit.update(cx, |this, cx| this.pick(window, cx)))
            .on_cancel(move |window, cx| on_cancel.update(cx, |this, cx| this.close(window, cx)))
    }

    fn row(&self, index: usize, target: &JumpTarget, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = Colors::active(cx);
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
            .child(div().w(px(48.)).text_color(colors.muted_text).child(kind))
            .child(div().flex_1().child(target.label.clone()))
            .child(
                div()
                    .text_color(colors.muted_text)
                    .child(target.detail.clone()),
            );
        if index == self.selected {
            row.bg(colors.current)
        } else {
            row
        }
    }
}

impl Render for JumpPalette {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = Colors::active(cx);
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
                    .on_key_down(cx.listener(Self::on_key_down))
                    .mt(px(80.))
                    .w(px(640.))
                    .h_auto()
                    .max_h(px(480.))
                    .flex()
                    .flex_col()
                    .bg(colors.background)
                    .border_1()
                    .border_color(colors.border)
                    .rounded_md()
                    .shadow_lg()
                    .font_family(TEXT_FONT)
                    .text_sm()
                    .text_color(colors.text)
                    .child(
                        div()
                            .p_3()
                            .border_b_1()
                            .border_color(colors.border)
                            .child(self.query_box(cx)),
                    )
                    .children(rows),
            )
            .into_any_element()
    }
}
