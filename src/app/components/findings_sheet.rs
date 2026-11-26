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
use crate::viewer::{Batch, Command, FindingLine, FindingsList, FindingsListRow, Presence};

use super::button::{ButtonKind, ButtonSize, button, icon_button};
use super::chip::chip;
use super::finding_view::tone_of;
use super::icon::{Icon, sized_icon};
use super::key_cap::{KeyLabel, key_hint};
use super::markdown_view::{InlineMarkdown, MarkdownView, parsed_markdown};
use super::motion::OVERLAY;
use super::overlay::{
    close_presence, modal_layer_for, overlay_panel, picker_footer, picker_row_at, rise_for,
    touching_places,
};
use super::palette::{CODE_FONT, CONTROL_RADIUS, Colors, Tone};
use super::sidebar_row::RowPlace;

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
    /// The row under the mouse, so the cursor row squares the corner it shares with it.
    hovered: Option<RowSpot>,
}

/// A row of the list: its place among the open rows or among the dismissed ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RowSpot {
    Open(usize),
    Dismissed(usize),
    DismissedHeader,
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
            hovered: None,
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
            self.hovered = None;
            self.presence.open(Instant::now());
            window.focus(&self.focus, cx);
            cx.notify();
        }
    }

    /// Nothing ticked, the third finding open and under the cursor, as a snapshot scene shows the list.
    #[cfg(feature = "snapshots")]
    pub fn stage_sample(&mut self, cx: &mut Context<Self>) {
        self.list.cursor_down();
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
            "right" => self.list.unfold_under_cursor(),
            "left" => self.list.fold_under_cursor(),
            "enter" => {
                if self.list.is_on_dismissed_header() {
                    self.list.toggle_dismissed_shown();
                } else if let Some(id) = self.list.id_under_cursor().cloned() {
                    self.close(window, cx);
                    cx.emit(FindingsEvent::GoTo(id));
                }
            }
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
            _ => {}
        }
        cx.notify();
    }

    fn ask(&mut self, event: fn(Vec<FindingId>) -> FindingsEvent, cx: &mut Context<Self>) {
        let targets = self.list.targets();
        if !targets.is_empty() {
            cx.emit(event(targets));
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
        spot: RowSpot,
        place: (RowPlace, Option<RowPlace>),
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let (tick_id, open_id, go_id) = (row.id.clone(), row.id.clone(), row.id.clone());
        let summary = picker_row_at(
            colors,
            SharedString::from(format!("findings-row-{}", row.id)),
            place.0,
            place.1,
        )
        .py_1p5()
        .on_hover(cx.listener(move |this, is_hovered: &bool, _, cx| {
            if *is_hovered {
                this.hovered = Some(spot);
            } else if this.hovered == Some(spot) {
                this.hovered = None;
            }
            cx.notify();
        }))
        .aria_label(format!("Open finding {}", row.id))
        .on_click(cx.listener(move |this, _, _, cx| {
            this.list.move_cursor_to(&open_id);
            this.list.toggle_open_under_cursor();
            cx.notify();
        }))
        .child(
            div()
                .id(SharedString::from(format!("findings-tick-{}", row.id)))
                .py_1()
                .role(gpui::Role::CheckBox)
                .aria_label("Select finding")
                .aria_toggled(if row.is_selected {
                    gpui::Toggled::True
                } else {
                    gpui::Toggled::False
                })
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
                .child(InlineMarkdown::new(
                    format!("findings-first-line-{}", row.id),
                    &row.first_line,
                )),
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
                                .aria_label("Show on its lines")
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
        spot: RowSpot,
        place: (RowPlace, Option<RowPlace>),
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let (tick_id, cursor_id, restore_id) = (row.id.clone(), row.id.clone(), row.id.clone());
        picker_row_at(
            colors,
            SharedString::from(format!("findings-dismissed-row-{}", row.id)),
            place.0,
            place.1,
        )
        .py_1p5()
        .on_hover(cx.listener(move |this, is_hovered: &bool, _, cx| {
            if *is_hovered {
                this.hovered = Some(spot);
            } else if this.hovered == Some(spot) {
                this.hovered = None;
            }
            cx.notify();
        }))
        .aria_label(format!("Open dismissed finding {}", row.id))
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
                .role(gpui::Role::CheckBox)
                .aria_label("Select dismissed finding")
                .aria_toggled(if row.is_selected {
                    gpui::Toggled::True
                } else {
                    gpui::Toggled::False
                })
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
            .aria_label("Restore")
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
        has_open_rows: bool,
        cursor: Option<RowSpot>,
        hovered: Option<RowSpot>,
        cx: &mut Context<Self>,
    ) -> Option<gpui::Div> {
        let rows = list.dismissed_rows();
        if rows.is_empty() {
            return None;
        }
        let places = places_of(&rows, RowSpot::Dismissed, cursor, hovered);
        let is_shown = list.dismissed_are_shown();
        let ticked = list.selected_dismissed().len();
        let place = if cursor == Some(RowSpot::DismissedHeader) {
            RowPlace::Current
        } else {
            RowPlace::Apart
        };
        let header = picker_row_at(colors, "findings-dismissed-header", place, None)
            .py_2()
            .gap_2()
            .aria_label("Show or hide dismissed findings")
            .aria_expanded(is_shown)
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
                    .child(with_count("Dismissed", rows.len())),
            )
            .when(is_shown, |header| {
                header.child(
                    Self::batch_button(
                        colors,
                        "findings-restore-selected",
                        with_count("Restore", ticked),
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
            div()
                .flex()
                .flex_col()
                .when(has_open_rows, |section| {
                    section
                        .mt_2()
                        .border_t_1()
                        .border_color(colors.subtle_border)
                        .pt_2()
                })
                .child(header)
                .children(
                    is_shown
                        .then(|| {
                            rows.iter()
                                .zip(places)
                                .enumerate()
                                .map(|(at, (row, place))| {
                                    Self::dismissed_row(
                                        colors,
                                        row,
                                        RowSpot::Dismissed(at),
                                        place,
                                        cx,
                                    )
                                })
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
        let base = button(colors, id, kind, ButtonSize::Small)
            .aria_label(label.clone())
            .child(label);
        match selection {
            Selection::Selected => base,
            Selection::Unselected => base.opacity(0.45).cursor_default(),
        }
    }
}

/// "Fix (3)", or just "Fix" while nothing is counted: a zero says nothing.
fn with_count(label: &str, count: usize) -> String {
    match count {
        0 => label.to_owned(),
        count => format!("{label} ({count})"),
    }
}

/// "Fix (3)" for three ticked rows, "Fix this" while none is ticked and the highlighted row is what
/// it acts on.
fn batch_label(label: &str, batch: Batch) -> String {
    match batch {
        Batch::Ticked(ticked) => with_count(label, ticked),
        Batch::Highlighted => format!("{label} this"),
        Batch::Nothing => label.to_owned(),
    }
}

/// How each row touches the cursor row and the hovered one. A row with its body open has nothing
/// touching it below, nor has the row under it above.
fn places_of(
    rows: &[FindingsListRow],
    spot_of: fn(usize) -> RowSpot,
    cursor: Option<RowSpot>,
    hovered: Option<RowSpot>,
) -> Vec<(RowPlace, Option<RowPlace>)> {
    rows.iter()
        .enumerate()
        .map(|(at, row)| {
            let Some(cursor) = cursor else {
                return (RowPlace::Apart, None);
            };
            let above = at
                .checked_sub(1)
                .filter(|above| rows.get(*above).is_some_and(|row| !row.is_open))
                .map(spot_of);
            let below = Some(at + 1)
                .filter(|below| !row.is_open && rows.get(*below).is_some())
                .map(spot_of);
            touching_places(spot_of(at), above, below, cursor, hovered)
        })
        .collect()
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
        let batch = self.list.batch();
        let selection = Selection::of(batch != Batch::Nothing);
        let close_keys: Vec<KeyLabel> = std::iter::once(KeyLabel::Text("esc"))
            .chain(
                keymap(cx)
                    .first_chord(Command::FindingsList)
                    .into_iter()
                    .flat_map(|chord| chord.caps().map(KeyLabel::of)),
            )
            .collect();
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
                    .child(with_count("Findings", rows.len())),
            )
            .child(
                div()
                    .id("findings-select-all")
                    .flex()
                    .items_center()
                    .gap_2()
                    .cursor_pointer()
                    .role(gpui::Role::CheckBox)
                    .aria_label("Select all")
                    .aria_toggled(if all_selected {
                        gpui::Toggled::True
                    } else {
                        gpui::Toggled::False
                    })
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
                    batch_label("Dismiss", batch),
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
                    batch_label("Fix", batch),
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
                    batch_label("Comment", batch),
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
        let dismissed = self.list.dismissed_rows();
        let cursor = rows
            .iter()
            .position(|row| row.is_under_cursor)
            .map(RowSpot::Open)
            .or_else(|| {
                dismissed
                    .iter()
                    .position(|row| row.is_under_cursor)
                    .map(RowSpot::Dismissed)
            })
            .or_else(|| {
                self.list
                    .is_on_dismissed_header()
                    .then_some(RowSpot::DismissedHeader)
            });
        let places = places_of(&rows, RowSpot::Open, cursor, self.hovered);
        let body = div()
            .id("findings-rows")
            .max_h(px(440.))
            .overflow_y_scroll()
            .pb_2()
            .when(!rows.is_empty(), Styled::pt_2)
            .flex()
            .flex_col()
            .children(
                rows.iter()
                    .zip(places)
                    .enumerate()
                    .map(|(at, (row, place))| Self::row(&colors, row, RowSpot::Open(at), place, cx))
                    .collect::<Vec<_>>(),
            )
            .children(Self::dismissed_section(
                &colors,
                &self.list,
                !rows.is_empty(),
                cursor,
                self.hovered,
                cx,
            ));
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
                    .w(px(880.))
                    .child(header)
                    .child(body)
                    .child(
                        picker_footer(&colors)
                            .child(key_hint(&colors, &close_keys, "close"))
                            .child(key_hint(&colors, &[KeyLabel::Text("space")], "tick"))
                            .child(key_hint(
                                &colors,
                                &[
                                    KeyLabel::Icon(Icon::ArrowLeft),
                                    KeyLabel::Icon(Icon::ArrowRight),
                                ],
                                "fold",
                            ))
                            .child(key_hint(&colors, &[KeyLabel::Text("return")], "show"))
                            .child(key_hint(&colors, &[KeyLabel::Text("a")], "all"))
                            .child(key_hint(&colors, &[KeyLabel::Text("d")], "dismiss"))
                            .child(key_hint(&colors, &[KeyLabel::Text("c")], "comment"))
                            .child(key_hint(&colors, &[KeyLabel::Text("x")], "fix"))
                            .child(key_hint(&colors, &[KeyLabel::Text("r")], "restore")),
                    ),
                "findings-sheet-rise",
                &self.presence,
            ))
            .into_any_element()
    }
}
