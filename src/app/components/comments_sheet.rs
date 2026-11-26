//! The comments list: every review thread as one compact row, to find a thread among the changes and
//! go to it. The window does the going, since only it knows the screens.

use std::time::Instant;

use gpui::prelude::FluentBuilder;
use gpui::{
    AnyElement, Context, EventEmitter, FocusHandle, FontWeight, InteractiveElement, IntoElement,
    KeyDownEvent, MouseButton, ParentElement, Render, ScrollHandle, SharedString,
    StatefulInteractiveElement, Styled, Window, div, px,
};

use crate::app::keymap::{chord_of, keymap};
use crate::viewer::{Command, CommentsList, CommentsListRow, CommentsSection, Presence, ThreadId};

use super::button::{ButtonKind, ButtonSize, icon_button};
use super::chip::chip;
use super::icon::{Icon, sized_icon};
use super::key_cap::{KeyLabel, key_hint};
use super::motion::OVERLAY;
use super::overlay::{
    close_presence, modal_layer_for, overlay_panel, picker_footer, picker_row_at, rise_for,
    touching_places,
};
use super::palette::{CODE_FONT, Colors, Tone};
use super::search_row::search_row;
use super::sidebar_row::RowPlace;
use super::text_box::TextBox;
use super::text_input::Lines;
use super::thread_view::author_avatar;

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
    scroll: ScrollHandle,
    /// The keys moved the cursor: the next draw scrolls its row into view.
    cursor_moved: bool,
    /// What the search box holds; `?` gives it the keys.
    query: String,
    search_focus: FocusHandle,
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
            scroll: ScrollHandle::new(),
            cursor_moved: false,
            query: String::new(),
            search_focus: cx.focus_handle(),
        }
    }

    pub fn toggle(&mut self, list: CommentsList, window: &mut Window, cx: &mut Context<Self>) {
        if self.presence.is_open() {
            self.close(window, cx);
        } else {
            self.list = list;
            self.query.clear();
            self.cursor_moved = true;
            self.presence.open(Instant::now());
            window.focus(&self.focus, cx);
            cx.notify();
        }
    }

    pub fn is_open(&self) -> bool {
        self.presence.is_open()
    }

    /// The threads changed while the list is open: it shows them, the cursor kept on its thread.
    pub fn refresh(&mut self, list: &CommentsList, cx: &mut Context<Self>) {
        if !self.presence.is_open() {
            return;
        }
        self.list = self.list.refreshed_to(list);
        self.list.set_query(&self.query);
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

    fn open_thread(&mut self, id: ThreadId, window: &mut Window, cx: &mut Context<Self>) {
        self.close(window, cx);
        cx.emit(CommentsEvent::Open(id));
    }

    /// While the search box has the keys, only Up and Down are the list's; the box takes the rest.
    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.search_focus.is_focused(window) {
            match event.keystroke.key.as_str() {
                "down" => self.list.cursor_down(),
                "up" => self.list.cursor_up(),
                _ => return,
            }
            self.cursor_moved = true;
            cx.stop_propagation();
            cx.notify();
            return;
        }
        if event.keystroke.key_char.as_deref() == Some("?") || event.keystroke.key == "?" {
            cx.stop_propagation();
            window.focus(&self.search_focus, cx);
            cx.notify();
            return;
        }
        let opens_the_sheet = chord_of(&event.keystroke)
            .is_some_and(|chord| keymap(cx).chords(Command::CommentsList).contains(&chord));
        cx.stop_propagation();
        match event.keystroke.key.as_str() {
            "escape" => self.close(window, cx),
            _ if opens_the_sheet => self.close(window, cx),
            "down" | "j" => {
                self.list.cursor_down();
                self.cursor_moved = true;
            }
            "up" | "k" => {
                self.list.cursor_up();
                self.cursor_moved = true;
            }
            "right" => self.list.unfold_under_cursor(),
            "left" => self.list.fold_under_cursor(),
            "enter" => {
                if self.list.is_on_resolved_header() {
                    self.list.toggle_resolved_shown();
                } else if let Some(id) = self.list.thread_under_cursor().cloned() {
                    self.open_thread(id, window, cx);
                }
            }
            _ => {}
        }
        cx.notify();
    }

    fn open_under_cursor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(id) = self.list.thread_under_cursor().cloned() {
            self.open_thread(id, window, cx);
        }
    }

    fn search_box(&self, colors: &Colors, window: &Window, cx: &mut Context<Self>) -> gpui::Div {
        let (on_edit, on_submit, on_cancel) = (cx.entity(), cx.entity(), cx.entity());
        let is_searching = self.search_focus.is_focused(window);
        let query_box = TextBox::new(
            "comments-search",
            &self.search_focus,
            &self.query,
            Lines::Single,
        )
        .label("Search comments")
        .on_edit(move |query, _, cx| {
            on_edit.update(cx, |this, cx| {
                this.list.set_query(&query);
                this.query = query;
                this.cursor_moved = true;
                cx.notify();
            });
        })
        .on_submit(move |window, cx| {
            on_submit.update(cx, |this, cx| this.open_under_cursor(window, cx));
        })
        .on_cancel(move |window, cx| {
            on_cancel.update(cx, |this, cx| {
                this.query.clear();
                this.list.set_query("");
                this.cursor_moved = true;
                window.focus(&this.focus, cx);
                cx.notify();
            });
        });
        search_row(
            colors,
            query_box,
            "Search comments by file, author or words",
            self.query.is_empty() && !is_searching,
            (!is_searching).then_some(KeyLabel::Text("?")),
        )
    }

    /// Puts the section's heading, its screen headings and its rows into `children` one after another,
    /// so the body's scroll handle counts them; `cursor_child` learns which child the cursor row is.
    fn section(
        colors: &Colors,
        section: &CommentsSection,
        selected: Option<usize>,
        next_position: &mut usize,
        into: (&mut Vec<AnyElement>, &mut Option<usize>),
        cx: &mut Context<Self>,
    ) {
        let (children, cursor_child) = into;
        if section.is_resolved {
            if section.is_header_under_cursor {
                *cursor_child = Some(children.len());
            }
            children.push(Self::resolved_header(colors, section, cx).into_any_element());
        } else {
            children.push(
                div()
                    .flex_none()
                    .mx_2()
                    .px_2()
                    .pt_3()
                    .pb_1()
                    .text_sm()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(colors.text)
                    .child(section.title.clone())
                    .into_any_element(),
            );
        }
        if !section.is_shown {
            return;
        }
        for group in &section.groups {
            let first = *next_position;
            *next_position += group.rows.len();
            let last = *next_position;
            children.push(
                div()
                    .flex_none()
                    .mx_2()
                    .px_2()
                    .pt_3()
                    .pb_1p5()
                    .truncate()
                    .text_xs()
                    .text_color(colors.muted_text)
                    .child(group.heading.clone())
                    .into_any_element(),
            );
            for (nth, row) in group.rows.iter().enumerate() {
                let position = first + nth;
                let above = (nth > 0).then(|| position - 1);
                let below = (position + 1 < last).then_some(position + 1);
                let (place, _) =
                    touching_places(position, above, below, selected.unwrap_or(usize::MAX), None);
                if selected == Some(position) {
                    *cursor_child = Some(children.len());
                }
                children.push(Self::row(colors, row, place, cx).into_any_element());
            }
        }
    }

    /// The fold of the resolved threads: a stop of the cursor, with a chevron that shows whether they show.
    fn resolved_header(
        colors: &Colors,
        section: &CommentsSection,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let place = if section.is_header_under_cursor {
            RowPlace::Current
        } else {
            RowPlace::Apart
        };
        picker_row_at(colors, "comments-resolved-header", place, None)
            .flex_none()
            .mt_2()
            .py_2()
            .gap_2()
            .aria_label("Show or hide resolved comments")
            .on_click(cx.listener(|this, _, _, cx| {
                this.list.toggle_resolved_shown();
                cx.notify();
            }))
            .child(sized_icon(
                if section.is_shown {
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
                    .text_sm()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(colors.muted_text)
                    .child(section.title.clone()),
            )
    }

    /// Every row has the same slots, so the columns line up: who, what they said, where, replies, Outdated.
    fn row(
        colors: &Colors,
        row: &CommentsListRow,
        place: RowPlace,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let id = row.id.clone();
        let text = if row.is_resolved {
            colors.muted_text
        } else {
            colors.text
        };
        let first_line = div()
            .flex_1()
            .min_w_0()
            .truncate()
            .child(row.first_line.clone());
        let first_line = if row.is_suggestion {
            first_line.italic().text_color(colors.muted_text)
        } else {
            first_line.text_color(text)
        };
        picker_row_at(
            colors,
            SharedString::from(format!("comments-row-{}", row.id)),
            place,
            None,
        )
        .flex_none()
        .py_2()
        .aria_label(row.first_line.clone())
        .on_click(cx.listener(move |this, _, window, cx| {
            this.open_thread(id.clone(), window, cx);
        }))
        .child(
            div()
                .flex_none()
                .size(px(AVATAR_SLOT))
                .flex()
                .items_center()
                .justify_center()
                .child(author_avatar(
                    colors,
                    &row.author,
                    row.avatar_url.as_deref(),
                    row.is_by_agent,
                    cx,
                )),
        )
        .child(
            div()
                .flex_none()
                .w(px(AUTHOR_WIDTH))
                .truncate()
                .font_weight(FontWeight::MEDIUM)
                .text_color(text)
                .child(row.author.clone()),
        )
        .child(first_line)
        .child(
            div()
                .flex_none()
                .w(px(PLACE_WIDTH))
                .truncate()
                .font_family(*CODE_FONT)
                .text_xs()
                .text_color(colors.muted_text)
                .child(row.place.clone()),
        )
        .child(
            div()
                .flex_none()
                .w(px(REPLIES_WIDTH))
                .flex()
                .justify_end()
                .children((row.replies > 0).then(|| {
                    chip(colors, Tone::Neutral)
                        .child(sized_icon(Icon::MessageSquare, colors.muted_text, px(11.)))
                        .child(row.replies.to_string())
                })),
        )
        .child(
            div()
                .flex_none()
                .w(px(OUTDATED_WIDTH))
                .flex()
                .justify_end()
                .children(
                    row.is_outdated
                        .then(|| chip(colors, Tone::Warning).child("Outdated")),
                ),
        )
    }
}

/// The avatar's own box, so a picture still loading or the agent mark never moves the row's text.
const AVATAR_SLOT: f32 = 24.;
const AUTHOR_WIDTH: f32 = 110.;
/// Wide enough for the 34 characters the viewer cuts a place to, in the code font at its small size.
const PLACE_WIDTH: f32 = 250.;
const REPLIES_WIDTH: f32 = 48.;
const OUTDATED_WIDTH: f32 = 76.;

impl Render for CommentsSheet {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.presence.shown().is_none() {
            return div().into_any_element();
        }
        let colors = Colors::active(cx);
        let rows = self.list.rows();
        let sections = self.list.sections();
        let selected = rows.iter().position(|row| row.is_under_cursor);
        let mut next_position = 0;
        let close_keys: Vec<KeyLabel> = std::iter::once(KeyLabel::Text("esc"))
            .chain(
                keymap(cx)
                    .first_chord(Command::CommentsList)
                    .into_iter()
                    .flat_map(|chord| chord.caps().map(KeyLabel::of)),
            )
            .collect();
        let title = format!("Comments ({})", self.list.thread_count());
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
        let mut children: Vec<AnyElement> = Vec::new();
        let mut cursor_child = None;
        for section in &sections {
            Self::section(
                &colors,
                section,
                selected,
                &mut next_position,
                (&mut children, &mut cursor_child),
                cx,
            );
        }
        if self.cursor_moved
            && let Some(child) = cursor_child
        {
            self.scroll.scroll_to_item(child);
            self.cursor_moved = false;
        }
        let body = div()
            .id("comments-rows")
            .max_h(px(440.))
            .overflow_y_scroll()
            .pb_2()
            .when(!rows.is_empty(), Styled::pt_2)
            .flex()
            .flex_col()
            .track_scroll(&self.scroll)
            .when(children.is_empty(), |rows| {
                rows.child(
                    div()
                        .px_4()
                        .py_3()
                        .text_color(colors.muted_text)
                        .child("No thread holds every word."),
                )
            })
            .children(children);
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
                    .child(self.search_box(&colors, window, cx))
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
                            .when(self.list.has_resolved(), |footer| {
                                footer.child(key_hint(
                                    &colors,
                                    &[
                                        KeyLabel::Icon(Icon::ArrowLeft),
                                        KeyLabel::Icon(Icon::ArrowRight),
                                    ],
                                    "fold resolved",
                                ))
                            })
                            .child(key_hint(&colors, &[KeyLabel::Text("return")], "show"))
                            .child(key_hint(&colors, &[KeyLabel::Text("?")], "search")),
                    ),
                "comments-sheet-rise",
                &self.presence,
            ))
            .into_any_element()
    }
}
