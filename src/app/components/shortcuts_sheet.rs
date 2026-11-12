use std::time::Instant;

use gpui::prelude::FluentBuilder;
use gpui::{
    Context, FocusHandle, FontWeight, InteractiveElement, IntoElement, KeyDownEvent, MouseButton,
    ParentElement, Render, SharedString, StatefulInteractiveElement, Styled, Window, div, px,
};

use crate::viewer::{Command, Presence, ShortcutGroup, in_group};

use super::button::{ButtonKind, ButtonSize, icon_button};
use super::icon::{Icon, sized_icon};
use super::key_cap::{CapGround, KeyLabel, chord_caps, key_hint};
use super::motion::OVERLAY;
use super::overlay::{close_presence, modal_layer_for, overlay_panel, picker_footer, rise_for};
use super::palette::Colors;
use super::step_notes::note_label;

/// Every shortcut the app has, under its heading, read from the one table the keys are bound from.
pub struct ShortcutsSheet {
    presence: Presence,
    focus: FocusHandle,
    owner_focus: FocusHandle,
}

impl ShortcutsSheet {
    pub const KEY_CONTEXT: &str = "ShortcutsSheet";

    pub fn new(owner_focus: FocusHandle, cx: &mut Context<Self>) -> Self {
        Self {
            presence: Presence::lasting(OVERLAY),
            focus: cx.focus_handle(),
            owner_focus,
        }
    }

    pub fn is_open(&self) -> bool {
        self.presence.is_open()
    }

    pub fn toggle(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.presence.is_open() {
            self.close(window, cx);
        } else {
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

    /// Esc closes it, and so does the key that opened it.
    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let is_question_mark = event.keystroke.key_char.as_deref() == Some("?");
        if event.keystroke.key == "escape" || is_question_mark {
            cx.stop_propagation();
            self.close(window, cx);
        }
    }
}

fn shortcut_row(colors: &Colors, command: Command) -> impl IntoElement {
    let chords = command.chords();
    let last = chords.len().saturating_sub(1);
    div()
        .min_h(px(28.))
        .flex()
        .items_center()
        .gap_3()
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_color(colors.text)
                .child(command.label()),
        )
        .child(div().flex().flex_none().items_center().gap_1p5().children(
            chords.iter().enumerate().map(move |(index, chord)| {
                div()
                    .flex()
                    .items_center()
                    .gap_1p5()
                    .child(chord_caps(colors, chord, CapGround::Page))
                    .when(index < last, |chord| {
                        chord.child(div().text_xs().text_color(colors.muted_text).child("or"))
                    })
            }),
        ))
}

fn group_block(colors: &Colors, group: ShortcutGroup) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap_1()
        .child(
            note_label(group.title())
                .pb_1()
                .text_color(colors.muted_text),
        )
        .children(in_group(group).map(|command| shortcut_row(colors, command)))
}

impl Render for ShortcutsSheet {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.presence.shown().is_none() {
            return div().into_any_element();
        }
        let colors = Colors::active(cx);
        let column = |groups: &[ShortcutGroup]| {
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap_5()
                .children(groups.iter().map(|group| group_block(&colors, *group)))
        };
        let header = div()
            .px_5()
            .pt_4()
            .pb_3()
            .flex()
            .items_center()
            .gap_3()
            .border_b_1()
            .border_color(colors.subtle_border)
            .child(sized_icon(Icon::Keyboard, colors.accent, px(18.)))
            .child(
                div()
                    .flex_1()
                    .text_base()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("Keyboard shortcuts"),
            )
            .child(
                icon_button(
                    &colors,
                    "shortcuts-close",
                    ButtonKind::Ghost,
                    ButtonSize::Small,
                    Icon::Close,
                    "Close",
                )
                .on_click(cx.listener(|this, _, window, cx| this.close(window, cx))),
            );
        modal_layer_for(&colors, "shortcuts-scrim", &self.presence)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| this.close(window, cx)),
            )
            .child(rise_for(
                overlay_panel(&colors)
                    .id("shortcuts-sheet")
                    .key_context(Self::KEY_CONTEXT)
                    .track_focus(&self.focus)
                    .on_key_down(cx.listener(Self::on_key_down))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .mt(px(80.))
                    .w(px(760.))
                    .child(header)
                    .child(
                        div()
                            .px_5()
                            .py_4()
                            .flex()
                            .gap_8()
                            .child(column(&[ShortcutGroup::Navigate, ShortcutGroup::Read]))
                            .child(column(&[
                                ShortcutGroup::Comment,
                                ShortcutGroup::View,
                                ShortcutGroup::App,
                            ])),
                    )
                    .child(
                        picker_footer(&colors)
                            .child(key_hint(&colors, &[KeyLabel::Text("esc")], "close"))
                            .child(div().flex_1())
                            .child(SharedString::from(
                                "In a palette: arrows move, return opens",
                            )),
                    ),
                "shortcuts-sheet-rise",
                &self.presence,
            ))
            .into_any_element()
    }
}
