use gpui::prelude::FluentBuilder;
use gpui::{
    Context, EventEmitter, FocusHandle, FontWeight, InteractiveElement, IntoElement, KeyDownEvent,
    ParentElement, Render, SharedString, StatefulInteractiveElement, Styled, Window, div, px,
};

use crate::pieces::PieceId;
use crate::viewer::{JumpKind, JumpTarget};

use super::markdown_view::InlineMarkdown;
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
                    this.query.clone_from(&query);
                    cx.emit(JumpEvent::QueryChanged(query));
                    cx.notify();
                });
            })
            .on_submit(move |window, cx| on_submit.update(cx, |this, cx| this.pick(window, cx)))
            .on_cancel(move |window, cx| on_cancel.update(cx, |this, cx| this.close(window, cx)))
    }

    fn row(&self, index: usize, target: &JumpTarget, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = Colors::active(cx);
        let is_selected = index == self.selected;
        let kind = match target.kind {
            JumpKind::Step => "Step",
            JumpKind::File => "File",
            JumpKind::Symbol => "Symbol",
        };
        let label = match target.kind {
            JumpKind::Step => div()
                .min_w_0()
                .truncate()
                .child(InlineMarkdown::new(
                    format!("jump-label-{index}"),
                    &target.label,
                ))
                .into_any_element(),
            JumpKind::File | JumpKind::Symbol => {
                code_label(&colors, &target.label).into_any_element()
            }
        };
        let detail = (!target.detail.is_empty()).then(|| {
            div()
                .flex_shrink(1.)
                .min_w(px(80.))
                .max_w(px(260.))
                .truncate()
                .text_xs()
                .text_color(colors.muted_text)
                .child(InlineMarkdown::new(
                    format!("jump-detail-{index}"),
                    &target.detail,
                ))
        });
        div()
            .id(SharedString::from(format!("jump-{index}")))
            .mx_2()
            .px_2()
            .h(px(32.))
            .flex()
            .items_center()
            .gap_3()
            .rounded_md()
            .overflow_hidden()
            .cursor_pointer()
            .map(|row| {
                if is_selected {
                    row.bg(colors.current)
                } else {
                    row.hover(|row| row.bg(colors.hover))
                }
            })
            .on_click(cx.listener(move |this, _, window, cx| {
                this.selected = index;
                this.pick(window, cx);
            }))
            .child(
                div()
                    .flex_none()
                    .w(px(52.))
                    .text_xs()
                    .text_color(if is_selected {
                        colors.accent
                    } else {
                        colors.muted_text
                    })
                    .child(kind),
            )
            .child(div().flex_1().min_w_0().overflow_hidden().child(label))
            .children(detail)
    }
}

/// A file or symbol target as `folder/` dim, then the file name, then the symbol, so the end of a
/// long path is what stays readable.
fn code_label(colors: &Colors, label: &str) -> impl IntoElement {
    let id = label.parse::<PieceId>().ok();
    let folder = id.as_ref().map_or("", PieceId::folder).to_owned();
    let file = id
        .as_ref()
        .map_or_else(|| label.to_owned(), |id| id.file_name().to_owned());
    let symbol = id
        .as_ref()
        .and_then(PieceId::symbol_path)
        .map(str::to_owned);
    let symbol_name = id
        .as_ref()
        .and_then(PieceId::symbol_name)
        .map(str::to_owned);
    div()
        .flex()
        .items_center()
        .gap_2()
        .min_w_0()
        .whitespace_nowrap()
        .children(symbol_name.map(|name| {
            div()
                .flex_none()
                .font_weight(FontWeight::MEDIUM)
                .child(SharedString::from(name))
        }))
        .child(
            div()
                .flex()
                .min_w_0()
                .overflow_hidden()
                .when(symbol.is_some(), Styled::text_xs)
                .child(
                    div()
                        .min_w_0()
                        .overflow_hidden()
                        .text_ellipsis_start()
                        .text_color(colors.muted_text)
                        .child(SharedString::from(folder)),
                )
                .child(
                    div()
                        .flex_none()
                        .when(symbol.is_some(), |file| file.text_color(colors.muted_text))
                        .child(SharedString::from(file)),
                ),
        )
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
            .items_start()
            .justify_center()
            .bg(gpui::rgba(0x00000066))
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|this, _, window, cx| this.close(window, cx)),
            )
            .child(
                div()
                    .id("jump-palette")
                    .key_context(Self::KEY_CONTEXT)
                    .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_key_down(cx.listener(Self::on_key_down))
                    .mt(px(80.))
                    .w(px(720.))
                    .h_auto()
                    .flex()
                    .flex_col()
                    .overflow_hidden()
                    .bg(colors.background)
                    .border_1()
                    .border_color(colors.border)
                    .rounded_lg()
                    .shadow_lg()
                    .font_family(TEXT_FONT)
                    .text_sm()
                    .text_color(colors.text)
                    .child(
                        div()
                            .px_4()
                            .py_3()
                            .text_base()
                            .border_b_1()
                            .border_color(colors.border)
                            .child(self.query_box(cx)),
                    )
                    .child(div().py_2().flex().flex_col().children(rows).when(
                        self.targets.is_empty(),
                        |list| {
                            list.child(
                                div().px_4().py_3().text_color(colors.muted_text).child(
                                    "Nothing matches. Try part of a file, symbol or step name.",
                                ),
                            )
                        },
                    ))
                    .child(
                        div()
                            .px_4()
                            .py_2()
                            .flex()
                            .gap_4()
                            .border_t_1()
                            .border_color(colors.subtle_border)
                            .bg(colors.surface)
                            .rounded_b_md()
                            .text_xs()
                            .text_color(colors.muted_text)
                            .child("↑ ↓  move")
                            .child("↵  open")
                            .child("esc  close"),
                    ),
            )
            .into_any_element()
    }
}
