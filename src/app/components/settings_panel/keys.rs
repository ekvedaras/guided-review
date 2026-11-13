//! The Keys section: every command with the keys it answers to, and a way to press new ones.

use gpui::prelude::FluentBuilder;
use gpui::{
    Context, FontWeight, IntoElement, ParentElement, SharedString, StatefulInteractiveElement,
    Styled, Subscription, div, px,
};

use crate::app::keymap::chord_of;
use crate::viewer::{Chord, Command, KeyProblem, ShortcutGroup, in_group};

use super::super::button::{ButtonKind, ButtonSize, button};
use super::super::icon::{Icon, sized_icon};
use super::super::key_cap::{CapGround, chord_caps};
use super::super::palette::{CONTROL_RADIUS, Colors};
use super::super::step_notes::note_label;
use super::{Editing, SettingsPanel};

/// The command whose next key press is taken as its new key, for as long as the interceptor lives.
pub(super) struct Recording {
    command: Command,
    _interceptor: Subscription,
}

impl SettingsPanel {
    pub(super) fn start_recording(&mut self, command: Command, cx: &mut Context<Self>) {
        let panel = cx.entity();
        let interceptor = cx.intercept_keystrokes(move |event, _, cx| {
            cx.stop_propagation();
            let keystroke = &event.keystroke;
            if keystroke.key == "escape" {
                panel.update(cx, Self::stop_recording);
                return;
            }
            if let Some(chord) = chord_of(keystroke) {
                panel.update(cx, |this, cx| this.record(chord, cx));
            }
        });
        self.recording = Some(Recording {
            command,
            _interceptor: interceptor,
        });
        cx.notify();
    }

    pub(super) fn stop_recording(&mut self, cx: &mut Context<Self>) {
        if self.recording.take().is_some() {
            cx.notify();
        }
    }

    fn record(&mut self, chord: Chord, cx: &mut Context<Self>) {
        let Some(Recording { command, .. }) = self.recording.take() else {
            return;
        };
        self.edit(cx, |editing| editing.settings.set_key(command, chord));
    }

    pub(super) fn keys(
        &self,
        editing: &Editing,
        colors: &Colors,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let problems = editing.settings.key_problems();
        let not_readable = |command: Command| {
            problems.iter().any(|problem| {
                matches!(problem, KeyProblem::NotAKey { command: it, .. } if *it == command)
            })
        };
        let groups = ShortcutGroup::ALL.into_iter().map(|group| {
            let rows = in_group(group).enumerate().map(|(at, command)| {
                let is_changed = editing.settings.is_key_changed(command) || not_readable(command);
                self.key_row(editing, colors, command, at > 0, is_changed, cx)
            });
            div()
                .flex()
                .flex_col()
                .gap_2()
                .child(note_label(group.title()).text_color(colors.muted_text))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .rounded(px(CONTROL_RADIUS))
                        .border_1()
                        .border_color(colors.subtle_border)
                        .bg(colors.background)
                        .children(rows),
                )
        });
        div()
            .flex()
            .flex_col()
            .gap_4()
            .children(self.key_problem_lines(&problems, colors, cx))
            .children(groups)
    }

    fn key_problem_lines(
        &self,
        problems: &[KeyProblem],
        colors: &Colors,
        cx: &Context<Self>,
    ) -> Option<gpui::Div> {
        (!problems.is_empty()).then(|| {
            div()
                .flex()
                .flex_col()
                .gap_1p5()
                .children(problems.iter().enumerate().map(|(at, problem)| {
                    let remove = match problem {
                        KeyProblem::UnknownCommand(name) => Some(name.clone()),
                        _ => None,
                    };
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .text_xs()
                        .text_color(colors.danger)
                        .child(sized_icon(Icon::CircleAlert, colors.danger, px(13.)))
                        .child(div().min_w_0().child(SharedString::from(problem.message())))
                        .children(remove.map(|name| {
                            button(
                                colors,
                                SharedString::from(format!("key-remove-{at}")),
                                ButtonKind::Danger,
                                ButtonSize::Small,
                            )
                            .child("Remove")
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    this.edit(cx, |editing| {
                                        editing.settings.remove_unknown_key(&name);
                                    });
                                },
                            ))
                        }))
                }))
        })
    }

    fn key_row(
        &self,
        editing: &Editing,
        colors: &Colors,
        command: Command,
        has_rule_above: bool,
        is_changed: bool,
        cx: &Context<Self>,
    ) -> gpui::Div {
        let name = command.config_name();
        let is_recording = self
            .recording
            .as_ref()
            .is_some_and(|recording| recording.command == command);
        let chords = editing.settings.key_chords(command);
        let keys = if is_recording {
            div()
                .text_color(colors.accent)
                .font_weight(FontWeight::MEDIUM)
                .child("Press the new key")
                .into_any_element()
        } else if chords.is_empty() {
            div()
                .text_color(colors.muted_text)
                .child("No key")
                .into_any_element()
        } else {
            div()
                .flex()
                .items_center()
                .gap_1p5()
                .children(chords.into_iter().enumerate().map(|(at, chord)| {
                    div()
                        .flex()
                        .items_center()
                        .gap_1p5()
                        .when(at > 0, |joined| {
                            joined.child(div().text_xs().text_color(colors.muted_text).child("or"))
                        })
                        .child(chord_caps(colors, chord, CapGround::Page))
                }))
                .into_any_element()
        };
        let change = if is_recording {
            button(
                colors,
                SharedString::from(format!("key-cancel-{name}")),
                ButtonKind::Secondary,
                ButtonSize::Small,
            )
            .child("Cancel")
            .on_click(cx.listener(|this, _, _, cx| this.stop_recording(cx)))
        } else {
            button(
                colors,
                SharedString::from(format!("key-change-{name}")),
                ButtonKind::Secondary,
                ButtonSize::Small,
            )
            .child("Change")
            .on_click(cx.listener(move |this, _, _, cx| this.start_recording(command, cx)))
        };
        let reset = is_changed.then(|| {
            button(
                colors,
                SharedString::from(format!("key-reset-{name}")),
                ButtonKind::Ghost,
                ButtonSize::Small,
            )
            .child("Reset")
            .on_click(cx.listener(move |this, _, _, cx| {
                this.stop_recording(cx);
                this.edit(cx, |editing| editing.settings.reset_key(command));
            }))
        });
        div()
            .px_3()
            .py_2()
            .flex()
            .items_center()
            .gap_3()
            .when(has_rule_above, |row| {
                row.border_t_1().border_color(colors.subtle_border)
            })
            .child(div().flex_1().min_w_0().child(command.label()))
            .child(div().w(px(190.)).flex_none().child(keys))
            .child(
                div()
                    .w(px(140.))
                    .flex()
                    .flex_none()
                    .justify_end()
                    .gap_1()
                    .child(change)
                    .children(reset),
            )
    }
}
