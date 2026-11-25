//! The comments list: every review thread as one compact row, to find a thread among the changes and
//! go to it. The window does the going, since only it knows the screens.

use std::time::Instant;

use gpui::prelude::FluentBuilder;
use gpui::{
    Context, EventEmitter, FocusHandle, FontWeight, InteractiveElement, IntoElement, KeyDownEvent,
    MouseButton, ParentElement, Render, SharedString, StatefulInteractiveElement, Styled, Window,
    div, px,
};

use crate::app::keymap::{chord_of, keymap};
use crate::viewer::{Command, CommentsList, CommentsListRow, Presence, ThreadId};

use super::button::{ButtonKind, ButtonSize, icon_button};
use super::chip::chip;
use super::icon::{Icon, sized_icon};
use super::key_cap::{KeyLabel, key_hint};
use super::motion::OVERLAY;
use super::overlay::{
    close_presence, modal_layer_for, overlay_panel, picker_footer, picker_row_at, rise_for,
};
use super::palette::{CODE_FONT, Colors, Tone};
use super::sidebar_row::RowPlace;

/// What the reviewer asked of the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommentsEvent {
    /// Closes the list and shows the thread.
    Open(ThreadId),
}

pub struct CommentsSheet {
    presence: Presence,
    focus: FocusHandle,
    owner_focus: FocusHandle,
    list: CommentsList,
}

impl EventEmitter<CommentsEvent> for CommentsSheet {}

impl CommentsSheet {
    pub const KEY_CONTEXT: &str = "CommentsSheet";

    pub fn new(owner_focus: FocusHandle, cx: &mut Context<Self>) -> Self {
        Self {
            presence: Presence::lasting(OVERLAY),
            focus: cx.focus_handle(),
            owner_focus,
            list: CommentsList::default(),
        }
    }

    pub fn toggle(&mut self, list: CommentsList, window: &mut Window, cx: &mut Context<Self>) {
        if self.presence.is_open() {
            self.close(window, cx);
        } else {
            self.list = list;
            self.presence.open(Instant::now());
            window.focus(&self.focus, cx);
            cx.notify();
        }
    }

    pub fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.presence.is_open() {
            return;
        }
        close_presence(self, cx, |this| &mut this.presence);
        window.focus(&self.owner_focus, cx);
        cx.notify();
    }

    fn open_thread(&mut self, id: ThreadId, window: &mut Window, cx: &mut Context<Self>) {
        self.close(window, cx);
        cx.emit(CommentsEvent::Open(id));
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let opens_the_sheet = chord_of(&event.keystroke)
            .is_some_and(|chord| keymap(cx).chords(Command::CommentsList).contains(&chord));
        cx.stop_propagation();
        match event.keystroke.key.as_str() {
            "escape" => self.close(window, cx),
            _ if opens_the_sheet => self.close(window, cx),
            "down" | "j" => self.list.cursor_down(),
            "up" | "k" => self.list.cursor_up(),
            "enter" => {
                if let Some(id) = self.list.thread_under_cursor().cloned() {
                    self.open_thread(id, window, cx);
                }
            }
            _ => {}
        }
        cx.notify();
    }

    fn row(
        colors: &Colors,
        row: &CommentsListRow,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let id = row.id.clone();
        let place = if row.is_under_cursor {
            RowPlace::Current
        } else {
            RowPlace::Apart
        };
        let text = if row.is_resolved {
            colors.muted_text
        } else {
            colors.text
        };
        picker_row_at(
            colors,
            SharedString::from(format!("comments-row-{}", row.id)),
            place,
            None,
        )
        .py_1p5()
        .when(row.is_resolved, |row| row.opacity(0.7))
        .on_click(cx.listener(move |this, _, window, cx| {
            this.open_thread(id.clone(), window, cx);
        }))
        .child(
            div()
                .flex_none()
                .w(px(80.))
                .truncate()
                .font_weight(FontWeight::MEDIUM)
                .text_color(text)
                .child(row.author.clone()),
        )
        .child(
            div()
                .flex_none()
                .w(px(310.))
                .truncate()
                .font_family(*CODE_FONT)
                .text_xs()
                .text_color(colors.muted_text)
                .child(row.place.clone()),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_color(text)
                .child(row.first_line.clone()),
        )
        .child(
            div()
                .flex_none()
                .w(px(150.))
                .flex()
                .justify_end()
                .gap_1()
                .children((row.replies > 0).then(|| {
                    chip(colors, Tone::Neutral)
                        .child(sized_icon(Icon::MessageSquare, colors.muted_text, px(11.)))
                        .child(row.replies.to_string())
                }))
                .children(
                    row.is_resolved
                        .then(|| chip(colors, Tone::Success).child("Resolved")),
                )
                .children(
                    row.is_outdated
                        .then(|| chip(colors, Tone::Warning).child("Outdated")),
                ),
        )
        .child(
            div()
                .flex_none()
                .w(px(140.))
                .truncate()
                .text_xs()
                .text_color(colors.muted_text)
                .child(
                    row.screen_title
                        .clone()
                        .unwrap_or_else(|| "Not in this tour".to_owned()),
                ),
        )
    }
}

impl Render for CommentsSheet {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.presence.shown().is_none() {
            return div().into_any_element();
        }
        let colors = Colors::active(cx);
        let rows = self.list.rows();
        let close_keys: Vec<KeyLabel> = std::iter::once(KeyLabel::Text("esc"))
            .chain(
                keymap(cx)
                    .first_chord(Command::CommentsList)
                    .into_iter()
                    .flat_map(|chord| chord.caps().map(KeyLabel::of)),
            )
            .collect();
        let title = format!("Comments ({})", rows.len());
        let header = div()
            .px_5()
            .pt_4()
            .pb_3()
            .flex()
            .items_center()
            .gap_3()
            .border_b_1()
            .border_color(colors.subtle_border)
            .child(sized_icon(Icon::MessageSquare, colors.accent, px(18.)))
            .child(
                div()
                    .flex_1()
                    .text_base()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(title),
            )
            .child(
                icon_button(
                    &colors,
                    "comments-close",
                    ButtonKind::Ghost,
                    ButtonSize::Small,
                    Icon::Close,
                    "Close",
                )
                .on_click(cx.listener(|this, _, window, cx| this.close(window, cx))),
            );
        let body = div()
            .id("comments-rows")
            .max_h(px(440.))
            .overflow_y_scroll()
            .pb_2()
            .when(!rows.is_empty(), Styled::pt_2)
            .flex()
            .flex_col()
            .children(
                rows.iter()
                    .map(|row| Self::row(&colors, row, cx))
                    .collect::<Vec<_>>(),
            );
        modal_layer_for(&colors, "comments-scrim", &self.presence)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| this.close(window, cx)),
            )
            .child(rise_for(
                overlay_panel(&colors)
                    .id("comments-sheet")
                    .key_context(Self::KEY_CONTEXT)
                    .track_focus(&self.focus)
                    .on_key_down(cx.listener(Self::on_key_down))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .mt(px(80.))
                    .w(px(1000.))
                    .child(header)
                    .child(body)
                    .child(
                        picker_footer(&colors)
                            .child(key_hint(&colors, &close_keys, "close"))
                            .child(key_hint(
                                &colors,
                                &[
                                    KeyLabel::Icon(Icon::ArrowUp),
                                    KeyLabel::Icon(Icon::ArrowDown),
                                ],
                                "move",
                            ))
                            .child(key_hint(&colors, &[KeyLabel::Text("return")], "show")),
                    ),
                "comments-sheet-rise",
                &self.presence,
            ))
            .into_any_element()
    }
}
