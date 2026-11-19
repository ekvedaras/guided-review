//! The findings list: every open finding of the AI review as one compact row, to tick, open and
//! decide in a batch. The window does what its buttons ask, since only it knows the review.

use std::time::Instant;

use gpui::prelude::FluentBuilder;
use gpui::{
    Context, EventEmitter, FocusHandle, FontWeight, InteractiveElement, IntoElement, KeyDownEvent,
    MouseButton, ParentElement, Render, SharedString, StatefulInteractiveElement, Styled, Window,
    div, px,
};

use crate::app::keymap::{chord_of, keymap};
use crate::findings::FindingId;
use crate::viewer::{Command, FindingLine, FindingsList, FindingsListRow, Presence};

use super::button::{ButtonKind, ButtonSize, button, icon_button};
use super::chip::chip;
use super::finding_view::tone_of;
use super::icon::{Icon, sized_icon};
use super::key_cap::{KeyLabel, key_hint};
use super::markdown_view::{MarkdownView, parsed_markdown};
use super::motion::OVERLAY;
use super::overlay::{
    close_presence, modal_layer_for, overlay_panel, picker_footer, picker_row, rise_for,
};
use super::palette::{CODE_FONT, CONTROL_RADIUS, Colors, Tone};

/// What the reviewer asked of the selected findings, or of one of them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FindingsEvent {
    Dismiss(Vec<FindingId>),
    Comment(Vec<FindingId>),
    /// To the tour agent, as one message in reading order.
    Fix(Vec<FindingId>),
    /// Dismissed findings are open again.
    Restore(Vec<FindingId>),
    /// Closes the list and shows the finding on its lines.
    GoTo(FindingId),
}

/// Whether a checkbox or a batch button has findings ticked behind it.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Selection {
    Selected,
    Unselected,
}

impl Selection {
    const fn of(is_selected: bool) -> Self {
        if is_selected {
            Self::Selected
        } else {
            Self::Unselected
        }
    }
}

pub struct FindingsSheet {
    presence: Presence,
    focus: FocusHandle,
    owner_focus: FocusHandle,
    list: FindingsList,
}

impl EventEmitter<FindingsEvent> for FindingsSheet {}

impl FindingsSheet {
    pub const KEY_CONTEXT: &str = "FindingsSheet";

    pub fn new(owner_focus: FocusHandle, cx: &mut Context<Self>) -> Self {
        Self {
            presence: Presence::lasting(OVERLAY),
            focus: cx.focus_handle(),
            owner_focus,
            list: FindingsList::default(),
        }
    }

    pub fn is_open(&self) -> bool {
        self.presence.is_open()
    }

    pub fn toggle(&mut self, list: FindingsList, window: &mut Window, cx: &mut Context<Self>) {
        if self.presence.is_open() {
            self.close(window, cx);
        } else {
            self.list = list;
            self.presence.open(Instant::now());
            window.focus(&self.focus, cx);
            cx.notify();
        }
    }

    /// The first two findings ticked and the third open, as a snapshot scene shows the list.
    #[cfg(feature = "snapshots")]
    pub fn stage_sample(&mut self, cx: &mut Context<Self>) {
        self.list.toggle_under_cursor();
        self.list.cursor_down();
        self.list.toggle_under_cursor();
        self.list.cursor_down();
        self.list.toggle_open_under_cursor();
        cx.notify();
    }

    /// The dismissed section unfolded, as a snapshot scene shows it.
    #[cfg(feature = "snapshots")]
    pub fn stage_dismissed_shown(&mut self, cx: &mut Context<Self>) {
        if !self.list.dismissed_are_shown() {
            self.list.toggle_dismissed_shown();
        }
        cx.notify();
    }

    /// The findings that are still open after a batch: what is ticked stays ticked, and an empty list closes.
    pub fn refresh(&mut self, list: &FindingsList, window: &mut Window, cx: &mut Context<Self>) {
        let remaining = self.list.refreshed_to(list);
        self.list = remaining;
        if self.list.is_empty() {
            self.close(window, cx);
        }
        cx.notify();
    }

    pub fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.presence.is_open() {
            return;
        }
        close_presence(self, cx, |this| &mut this.presence);
        window.focus(&self.owner_focus, cx);
        cx.notify();
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let key = event.keystroke.key.as_str();
        let opens_the_sheet = chord_of(&event.keystroke)
            .is_some_and(|chord| keymap(cx).chords(Command::FindingsList).contains(&chord));
        cx.stop_propagation();
        match key {
            "escape" => self.close(window, cx),
            _ if opens_the_sheet => self.close(window, cx),
            "down" | "j" => self.list.cursor_down(),
            "up" | "k" => self.list.cursor_up(),
            "space" => self.list.toggle_under_cursor(),
            "enter" => self.list.toggle_open_under_cursor(),
            "a" => self.list.toggle_all(),
            "d" => self.ask(FindingsEvent::Dismiss, cx),
            "c" => self.ask(FindingsEvent::Comment, cx),
            "x" => self.ask(FindingsEvent::Fix, cx),
            "r" => {
                let selected = self.list.selected_dismissed();
                if !selected.is_empty() {
                    cx.emit(FindingsEvent::Restore(selected));
                }
            }
            "h" => self.list.toggle_dismissed_shown(),
            "g" => {
                if let Some(id) = self.list.id_under_cursor().cloned() {
                    self.close(window, cx);
                    cx.emit(FindingsEvent::GoTo(id));
                }
            }
            _ => {}
        }
        cx.notify();
    }

    fn ask(&mut self, event: fn(Vec<FindingId>) -> FindingsEvent, cx: &mut Context<Self>) {
        let selected = self.list.selected();
        if !selected.is_empty() {
            cx.emit(event(selected));
        }
    }

    fn checkbox(colors: &Colors, selection: Selection) -> impl IntoElement {
        let is_selected = selection == Selection::Selected;
        div()
            .flex_none()
            .size(px(16.))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(4.))
            .border_1()
            .border_color(if is_selected {
                colors.accent
            } else {
                colors.border
            })
            .when(is_selected, |checkbox| {
                checkbox
                    .bg(colors.accent)
                    .child(sized_icon(Icon::Check, colors.overlay, px(12.)))
            })
    }

    fn row(
        colors: &Colors,
        row: &FindingsListRow,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let (tick_id, open_id, go_id) = (row.id.clone(), row.id.clone(), row.id.clone());
        let summary = picker_row(
            colors,
            SharedString::from(format!("findings-row-{}", row.id)),
            row.is_under_cursor,
        )
        .py_1p5()
        .on_click(cx.listener(move |this, _, _, cx| {
            this.list.move_cursor_to(&open_id);
            this.list.toggle_open_under_cursor();
            cx.notify();
        }))
        .child(
            div()
                .id(SharedString::from(format!("findings-tick-{}", row.id)))
                .py_1()
                .on_click(cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    this.list.move_cursor_to(&tick_id);
                    this.list.toggle_under_cursor();
                    cx.notify();
                }))
                .child(Self::checkbox(colors, Selection::of(row.is_selected))),
        )
        .child(chip(colors, tone_of(row.severity)).child(row.severity.to_string()))
        .children(
            row.is_outdated
                .then(|| chip(colors, Tone::Warning).child("Outdated")),
        )
        .child(
            div()
                .flex_none()
                .text_xs()
                .text_color(colors.muted_text)
                .child(row.place.clone()),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_color(colors.text)
                .child(row.first_line.clone()),
        )
        .children(row.fix.as_ref().map(|line| {
            div()
                .flex_none()
                .text_xs()
                .text_color(colors.muted_text)
                .child(line.clone())
        }))
        .child(sized_icon(
            if row.is_open {
                Icon::ChevronDown
            } else {
                Icon::ChevronRight
            },
            colors.muted_text,
            px(14.),
        ));
        div()
            .flex()
            .flex_col()
            .child(summary)
            .when(row.is_open, |rows| {
                rows.child(
                    div()
                        .mx_2()
                        .mb_2()
                        .ml(px(44.))
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(
                            MarkdownView::new(
                                format!("findings-body-{}", row.id),
                                parsed_markdown(&row.body),
                            )
                            .into_element(),
                        )
                        .children(code_block(colors, &row.lines))
                        .child(
                            div().flex().child(
                                button(
                                    colors,
                                    SharedString::from(format!("findings-go-{}", row.id)),
                                    ButtonKind::Ghost,
                                    ButtonSize::Small,
                                )
                                .child("Show on its lines")
                                .on_click(cx.listener(
                                    move |this, _, window, cx| {
                                        this.close(window, cx);
                                        cx.emit(FindingsEvent::GoTo(go_id.clone()));
                                    },
                                )),
                            ),
                        ),
                )
            })
    }

    /// A dismissed finding on one line, with the way back.
    fn dismissed_row(
        colors: &Colors,
        row: &FindingsListRow,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let (tick_id, cursor_id, restore_id) = (row.id.clone(), row.id.clone(), row.id.clone());
        picker_row(
            colors,
            SharedString::from(format!("findings-dismissed-row-{}", row.id)),
            row.is_under_cursor,
        )
        .py_1p5()
        .on_click(cx.listener(move |this, _, _, cx| {
            this.list.move_cursor_to_dismissed(&cursor_id);
            cx.notify();
        }))
        .child(
            div()
                .id(SharedString::from(format!(
                    "findings-dismissed-tick-{}",
                    row.id
                )))
                .py_1()
                .on_click(cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    this.list.move_cursor_to_dismissed(&tick_id);
                    this.list.toggle_dismissed_selected(&tick_id);
                    cx.notify();
                }))
                .child(Self::checkbox(colors, Selection::of(row.is_selected))),
        )
        .child(chip(colors, tone_of(row.severity)).child(row.severity.to_string()))
        .child(
            div()
                .flex_none()
                .text_xs()
                .text_color(colors.muted_text)
                .child(row.place.clone()),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_color(colors.muted_text)
                .child(row.first_line.clone()),
        )
        .child(
            button(
                colors,
                SharedString::from(format!("findings-restore-{}", row.id)),
                ButtonKind::Ghost,
                ButtonSize::Small,
            )
            .child("Restore")
            .on_click(cx.listener(move |_, _, _, cx| {
                cx.stop_propagation();
                cx.emit(FindingsEvent::Restore(vec![restore_id.clone()]));
            })),
        )
    }

    /// The fold with the dismissed findings under it, and the batch Restore while they show.
    fn dismissed_section(
        colors: &Colors,
        list: &FindingsList,
        cx: &mut Context<Self>,
    ) -> Option<gpui::Div> {
        let rows = list.dismissed_rows();
        if rows.is_empty() {
            return None;
        }
        let is_shown = list.dismissed_are_shown();
        let ticked = list.selected_dismissed().len();
        let header = div()
            .id("findings-dismissed-header")
            .px_5()
            .py_2()
            .flex()
            .items_center()
            .gap_2()
            .cursor_pointer()
            .border_t_1()
            .border_color(colors.subtle_border)
            .on_click(cx.listener(|this, _, _, cx| {
                this.list.toggle_dismissed_shown();
                cx.notify();
            }))
            .child(sized_icon(
                if is_shown {
                    Icon::ChevronDown
                } else {
                    Icon::ChevronRight
                },
                colors.muted_text,
                px(14.),
            ))
            .child(
                div()
                    .flex_1()
                    .text_color(colors.muted_text)
                    .child(format!("Dismissed ({})", rows.len())),
            )
            .when(is_shown, |header| {
                header.child(
                    Self::batch_button(
                        colors,
                        "findings-restore-selected",
                        format!("Restore ({ticked})"),
                        ButtonKind::Secondary,
                        Selection::of(ticked > 0),
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        cx.stop_propagation();
                        let selected = this.list.selected_dismissed();
                        if !selected.is_empty() {
                            cx.emit(FindingsEvent::Restore(selected));
                        }
                    })),
                )
            });
        Some(
            div().flex().flex_col().child(header).children(
                is_shown
                    .then(|| {
                        rows.iter()
                            .map(|row| Self::dismissed_row(colors, row, cx))
                            .collect::<Vec<_>>()
                    })
                    .into_iter()
                    .flatten(),
            ),
        )
    }

    fn batch_button(
        colors: &Colors,
        id: &'static str,
        label: String,
        kind: ButtonKind,
        selection: Selection,
    ) -> gpui::Stateful<gpui::Div> {
        let base = button(colors, id, kind, ButtonSize::Small).child(label);
        match selection {
            Selection::Selected => base,
            Selection::Unselected => base.opacity(0.45).cursor_default(),
        }
    }
}

/// The code lines a finding is about, with their line numbers; nothing for a finding on no lines.
fn code_block(colors: &Colors, lines: &[FindingLine]) -> Option<gpui::Div> {
    if lines.is_empty() {
        return None;
    }
    Some(
        div()
            .py_1()
            .px_2()
            .rounded(px(CONTROL_RADIUS))
            .bg(colors.subtle_border)
            .font_family(*CODE_FONT)
            .text_sm()
            .overflow_hidden()
            .children(lines.iter().map(|line| {
                div()
                    .flex()
                    .gap_3()
                    .whitespace_nowrap()
                    .child(
                        div()
                            .flex_none()
                            .text_color(colors.muted_text)
                            .child(line.number.to_string()),
                    )
                    .child(SharedString::from(line.text.clone()))
            })),
    )
}

impl Render for FindingsSheet {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.presence.shown().is_none() {
            return div().into_any_element();
        }
        let colors = Colors::active(cx);
        let rows = self.list.rows();
        let selected = self.list.selected().len();
        let selection = Selection::of(selected > 0);
        let all_selected = self.list.all_selected();
        let header = div()
            .px_5()
            .pt_4()
            .pb_3()
            .flex()
            .items_center()
            .gap_3()
            .border_b_1()
            .border_color(colors.subtle_border)
            .child(sized_icon(Icon::ListChecks, colors.accent, px(18.)))
            .child(
                div()
                    .flex_1()
                    .text_base()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(format!("Findings ({})", rows.len())),
            )
            .child(
                div()
                    .id("findings-select-all")
                    .flex()
                    .items_center()
                    .gap_2()
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.list.toggle_all();
                        cx.notify();
                    }))
                    .child(Self::checkbox(&colors, Selection::of(all_selected)))
                    .child("Select all"),
            )
            .child(
                Self::batch_button(
                    &colors,
                    "findings-dismiss",
                    format!("Dismiss ({selected})"),
                    ButtonKind::Secondary,
                    selection,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.ask(FindingsEvent::Dismiss, cx);
                })),
            )
            .child(
                Self::batch_button(
                    &colors,
                    "findings-fix",
                    format!("Fix ({selected})"),
                    ButtonKind::Secondary,
                    selection,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.ask(FindingsEvent::Fix, cx);
                })),
            )
            .child(
                Self::batch_button(
                    &colors,
                    "findings-comment",
                    format!("Comment ({selected})"),
                    ButtonKind::Primary,
                    selection,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.ask(FindingsEvent::Comment, cx);
                })),
            )
            .child(
                icon_button(
                    &colors,
                    "findings-close",
                    ButtonKind::Ghost,
                    ButtonSize::Small,
                    Icon::Close,
                    "Close",
                )
                .on_click(cx.listener(|this, _, window, cx| this.close(window, cx))),
            );
        let body = div()
            .id("findings-rows")
            .max_h(px(440.))
            .overflow_y_scroll()
            .py_2()
            .flex()
            .flex_col()
            .children(
                rows.iter()
                    .map(|row| Self::row(&colors, row, cx))
                    .collect::<Vec<_>>(),
            )
            .children(Self::dismissed_section(&colors, &self.list, cx));
        modal_layer_for(&colors, "findings-scrim", &self.presence)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| this.close(window, cx)),
            )
            .child(rise_for(
                overlay_panel(&colors)
                    .id("findings-sheet")
                    .key_context(Self::KEY_CONTEXT)
                    .track_focus(&self.focus)
                    .on_key_down(cx.listener(Self::on_key_down))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .mt(px(80.))
                    .w(px(820.))
                    .child(header)
                    .child(body)
                    .child(
                        picker_footer(&colors)
                            .child(key_hint(&colors, &[KeyLabel::Text("esc")], "close"))
                            .child(key_hint(&colors, &[KeyLabel::Text("space")], "tick"))
                            .child(key_hint(&colors, &[KeyLabel::Text("return")], "open"))
                            .child(key_hint(&colors, &[KeyLabel::Text("a")], "all"))
                            .child(key_hint(&colors, &[KeyLabel::Text("d")], "dismiss"))
                            .child(key_hint(&colors, &[KeyLabel::Text("c")], "comment"))
                            .child(key_hint(&colors, &[KeyLabel::Text("x")], "fix"))
                            .child(key_hint(&colors, &[KeyLabel::Text("h")], "dismissed"))
                            .child(key_hint(&colors, &[KeyLabel::Text("r")], "restore"))
                            .child(key_hint(&colors, &[KeyLabel::Text("g")], "show")),
                    ),
                "findings-sheet-rise",
                &self.presence,
            ))
            .into_any_element()
    }
}
