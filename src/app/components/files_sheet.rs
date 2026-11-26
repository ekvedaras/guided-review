//! The files list: every changed file as a folder tree with its line counts and read tick, and on the
//! right the screens that show the picked file. The window does the going, since only it scrolls.

use std::time::Instant;

use gpui::prelude::FluentBuilder;
use gpui::{
    AnyElement, Context, EventEmitter, FocusHandle, FontWeight, InteractiveElement, IntoElement,
    KeyDownEvent, MouseButton, ParentElement, Render, ScrollHandle, StatefulInteractiveElement,
    Styled, Window, div, px,
};

use crate::app::keymap::{chord_of, keymap};
use crate::viewer::{
    Command, FilePlace, FilesList, FilesListRow, FilesListRowKind, JumpTarget, ListedFile, Presence,
};

use super::button::{ButtonKind, ButtonSize, icon_button};
use super::icon::{Icon, sized_icon};
use super::key_cap::{KeyLabel, key_cap, key_hint};
use super::motion::OVERLAY;
use super::overlay::{
    close_presence, modal_layer_for, overlay_panel, picker_footer, picker_row_at, rise_for,
    touching_places,
};
use super::palette::{CODE_FONT, Colors};
use super::text_box::TextBox;
use super::text_input::Lines;

/// What the reviewer asked of the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FilesEvent {
    /// Closes the list and shows the file on that screen.
    Go(JumpTarget),
}

pub struct FilesSheet {
    presence: Presence,
    focus: FocusHandle,
    owner_focus: FocusHandle,
    list: FilesList,
    /// What the search box holds; `?` gives it the keys.
    query: String,
    search_focus: FocusHandle,
    scroll: ScrollHandle,
    /// The keys moved the cursor: the next draw scrolls its row into view.
    cursor_moved: bool,
}

impl EventEmitter<FilesEvent> for FilesSheet {}

/// Indent per level: a file's read mark then sits under its folder's icon, past the chevron and the gap.
const INDENT: f32 = 20.;
const DETAIL_WIDTH: f32 = 380.;
const COUNTS_WIDTH: f32 = 96.;

impl FilesSheet {
    pub const KEY_CONTEXT: &str = "FilesSheet";

    pub fn new(owner_focus: FocusHandle, cx: &mut Context<Self>) -> Self {
        Self {
            presence: Presence::lasting(OVERLAY),
            focus: cx.focus_handle(),
            owner_focus,
            list: FilesList::default(),
            query: String::new(),
            search_focus: cx.focus_handle(),
            scroll: ScrollHandle::new(),
            cursor_moved: false,
        }
    }

    pub fn toggle(&mut self, list: FilesList, window: &mut Window, cx: &mut Context<Self>) {
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

    /// The list as it is after typing `query` in its search box, for a picture.
    #[cfg(feature = "snapshots")]
    pub fn stage_query(&mut self, query: &str, window: &mut Window, cx: &mut Context<Self>) {
        query.clone_into(&mut self.query);
        self.list.set_query(query);
        self.cursor_moved = true;
        window.focus(&self.search_focus, cx);
        cx.notify();
    }

    pub fn is_open(&self) -> bool {
        self.presence.is_open()
    }

    /// The files were read again while the list is open: it shows them, its folds and cursor kept.
    pub fn refresh(&mut self, list: &FilesList, cx: &mut Context<Self>) {
        if !self.presence.is_open() {
            return;
        }
        self.list = self.list.refreshed_to(list);
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

    fn go(&mut self, nth: usize, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(target) = self.list.target_of_picked(nth) {
            self.close(window, cx);
            cx.emit(FilesEvent::Go(target));
        }
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
        let opens_the_sheet = chord_of(&event.keystroke)
            .is_some_and(|chord| keymap(cx).chords(Command::FilesList).contains(&chord));
        cx.stop_propagation();
        let key = event.keystroke.key.as_str();
        match key {
            "escape" => self.close(window, cx),
            _ if opens_the_sheet => self.close(window, cx),
            _ if event.keystroke.key_char.as_deref() == Some("?") || key == "?" => {
                window.focus(&self.search_focus, cx);
            }
            "down" | "j" => {
                self.list.cursor_down();
                self.cursor_moved = true;
            }
            "up" | "k" => {
                self.list.cursor_up();
                self.cursor_moved = true;
            }
            "right" | "l" => self.list.unfold_under_cursor(),
            "left" | "h" => self.list.fold_under_cursor(),
            "enter" if self.list.is_on_folder() => self.list.toggle_fold_under_cursor(),
            "enter" => self.go(0, window, cx),
            _ => {
                if let Some(nth) = key
                    .parse::<usize>()
                    .ok()
                    .and_then(|number| number.checked_sub(1))
                {
                    self.go(nth, window, cx);
                }
            }
        }
        cx.notify();
    }

    fn search_box(
        &self,
        colors: &Colors,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let (on_edit, on_submit, on_cancel) = (cx.entity(), cx.entity(), cx.entity());
        let is_searching = self.search_focus.is_focused(window);
        div()
            .flex_1()
            .min_w_0()
            .px_2()
            .py_1()
            .flex()
            .items_center()
            .gap_2()
            .rounded(px(6.))
            .border_1()
            .border_color(if is_searching {
                colors.accent
            } else {
                colors.border
            })
            .bg(colors.inset)
            .child(sized_icon(Icon::Search, colors.muted_text, px(14.)))
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_w_0()
                    .child(
                        TextBox::new(
                            "files-search",
                            &self.search_focus,
                            &self.query,
                            Lines::Single,
                        )
                        .label("Search changed files")
                        .on_edit(move |query, _, cx| {
                            on_edit.update(cx, |this, cx| {
                                this.list.set_query(&query);
                                this.query = query;
                                this.cursor_moved = true;
                                cx.notify();
                            });
                        })
                        .on_submit(move |window, cx| {
                            on_submit.update(cx, |this, cx| this.go(0, window, cx));
                        })
                        .on_cancel(move |window, cx| {
                            on_cancel.update(cx, |this, cx| {
                                this.query.clear();
                                this.list.set_query("");
                                this.cursor_moved = true;
                                window.focus(&this.focus, cx);
                                cx.notify();
                            });
                        }),
                    )
                    .when(self.query.is_empty() && !is_searching, |field| {
                        field.child(
                            div()
                                .absolute()
                                .top_0()
                                .left_0()
                                .text_color(colors.muted_text)
                                .child("Search files"),
                        )
                    }),
            )
            .children((!is_searching).then(|| key_cap(colors, KeyLabel::Text("?"))))
    }

    fn row(
        colors: &Colors,
        position: usize,
        row: &FilesListRow,
        selected: Option<usize>,
        count: usize,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let above = position.checked_sub(1);
        let below = (position + 1 < count).then_some(position + 1);
        let (place, _) =
            touching_places(position, above, below, selected.unwrap_or(usize::MAX), None);
        let base = picker_row_at(colors, ("files-row", position), place, None)
            .flex_none()
            .py_1()
            .gap_2()
            .pl(px(8. + INDENT * row.depth as f32))
            .aria_label(match &row.kind {
                FilesListRowKind::Folder { label, .. } | FilesListRowKind::File { label, .. } => {
                    label.clone()
                }
            })
            .on_click(cx.listener(move |this, _, _, cx| {
                this.list.put_cursor_on(position);
                this.list.toggle_fold_under_cursor();
                cx.notify();
            }));
        match &row.kind {
            FilesListRowKind::Folder {
                label,
                is_folded,
                added,
                removed,
                ..
            } => base
                .text_color(colors.sidebar_text)
                .child(sized_icon(
                    if *is_folded {
                        Icon::ChevronRight
                    } else {
                        Icon::ChevronDown
                    },
                    colors.muted_text,
                    px(12.),
                ))
                .child(sized_icon(
                    if *is_folded {
                        Icon::Folder
                    } else {
                        Icon::FolderOpen
                    },
                    colors.accent,
                    px(14.),
                ))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(label.clone()),
                )
                .child(line_counts(colors, *added, *removed)),
            FilesListRowKind::File {
                label,
                added,
                removed,
                is_read,
                place_count,
                ..
            } => base
                .text_color(if *is_read {
                    colors.muted_text
                } else {
                    colors.text
                })
                .child(read_mark(colors, *is_read))
                .child(div().flex_1().min_w_0().truncate().child(label.clone()))
                .children((*place_count > 1).then(|| {
                    div()
                        .flex_none()
                        .text_xs()
                        .text_color(colors.muted_text)
                        .child(format!("{place_count} steps"))
                }))
                .child(line_counts(colors, *added, *removed)),
        }
    }

    /// The picked file's path and the screens that show it, each a way to go there.
    fn detail(colors: &Colors, picked: Option<&ListedFile>, cx: &mut Context<Self>) -> AnyElement {
        let panel = div()
            .w(px(DETAIL_WIDTH))
            .flex_none()
            .border_l_1()
            .border_color(colors.subtle_border)
            .p_4()
            .flex()
            .flex_col()
            .gap_2();
        let Some(file) = picked else {
            return panel
                .text_color(colors.muted_text)
                .child("A folder: Return or a click folds it.")
                .into_any_element();
        };
        panel
            .child(
                div()
                    .font_family(*CODE_FONT)
                    .text_xs()
                    .text_color(colors.muted_text)
                    .child(file.path.to_string()),
            )
            .child(
                div()
                    .pt_2()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(shown_in(file)),
            )
            .children(
                file.places
                    .iter()
                    .enumerate()
                    .map(|(nth, place)| Self::place_row(colors, nth, place, cx)),
            )
            .into_any_element()
    }

    fn place_row(
        colors: &Colors,
        nth: usize,
        place: &FilePlace,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        div()
            .id(("files-place", nth))
            .px_2()
            .py_1p5()
            .flex()
            .items_center()
            .gap_2()
            .rounded(px(6.))
            .cursor_pointer()
            .hover(|row| row.bg(colors.hover))
            .when(place.is_current, |row| row.bg(colors.current))
            .role(gpui::Role::Button)
            .aria_label(place.heading.clone())
            .on_click(cx.listener(move |this, _, window, cx| this.go(nth, window, cx)))
            .child(read_mark(colors, place.is_read))
            .child(
                div()
                    .flex_none()
                    .w(px(14.))
                    .text_xs()
                    .text_color(colors.muted_text)
                    .children((nth < 9).then(|| (nth + 1).to_string())),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .child(place.heading.clone()),
            )
    }
}

fn shown_in(file: &ListedFile) -> String {
    match file.places.len() {
        0 => "No screen shows it".to_owned(),
        1 => "Shown on 1 screen".to_owned(),
        count => format!("Shown on {count} screens"),
    }
}

fn line_counts(colors: &Colors, added: usize, removed: usize) -> impl IntoElement {
    div()
        .flex_none()
        .w(px(COUNTS_WIDTH))
        .flex()
        .justify_end()
        .gap_1()
        .font_family(*CODE_FONT)
        .text_xs()
        .child(div().text_color(colors.success).child(format!("+{added}")))
        .child(div().text_color(colors.danger).child(format!("−{removed}")))
}

fn read_mark(colors: &Colors, is_read: bool) -> impl IntoElement {
    div()
        .flex_none()
        .size(px(14.))
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .border_1()
        .border_color(if is_read {
            colors.success
        } else {
            colors.border
        })
        .when(is_read, |mark| {
            mark.bg(colors.success_background).child(sized_icon(
                Icon::Check,
                colors.success,
                px(10.),
            ))
        })
}

impl Render for FilesSheet {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.presence.shown().is_none() {
            return div().into_any_element();
        }
        let colors = Colors::active(cx);
        let rows = self.list.rows();
        let selected = rows.iter().position(|row| row.is_under_cursor);
        let close_keys: Vec<KeyLabel> = std::iter::once(KeyLabel::Text("esc"))
            .chain(
                keymap(cx)
                    .first_chord(Command::FilesList)
                    .into_iter()
                    .flat_map(|chord| chord.caps().map(KeyLabel::of)),
            )
            .collect();
        let (added, removed) = self.list.line_counts();
        // Two parts as wide as the panes below, so the search box ends at the line between them.
        let header = div()
            .flex()
            .items_center()
            .border_b_1()
            .border_color(colors.subtle_border)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .pl_5()
                    .pr_4()
                    .pt_4()
                    .pb_3()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(sized_icon(Icon::FolderOpen, colors.accent, px(18.)))
                    .child(
                        div()
                            .flex_none()
                            .text_base()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(format!("Files ({})", self.list.file_count())),
                    )
                    .child(self.search_box(&colors, window, cx)),
            )
            .child(
                div()
                    .w(px(DETAIL_WIDTH))
                    .flex_none()
                    .pr_5()
                    .pt_4()
                    .pb_3()
                    .flex()
                    .items_center()
                    .justify_end()
                    .gap_3()
                    .child(line_counts(&colors, added, removed))
                    .child(
                        icon_button(
                            &colors,
                            "files-close",
                            ButtonKind::Ghost,
                            ButtonSize::Small,
                            Icon::Close,
                            "Close",
                        )
                        .on_click(cx.listener(|this, _, window, cx| this.close(window, cx))),
                    ),
            );
        let count = rows.len();
        let row_elements: Vec<AnyElement> = rows
            .iter()
            .enumerate()
            .map(|(position, row)| {
                Self::row(&colors, position, row, selected, count, cx).into_any_element()
            })
            .collect();
        // The rows have no bounds before their first layout, so a list that just opened scrolls on
        // the frame after.
        if self.cursor_moved
            && let Some(row) = selected
        {
            self.scroll.scroll_to_item(row);
            if self.scroll.bounds_for_item(row).is_some() {
                self.cursor_moved = false;
            } else {
                cx.notify();
            }
        }
        let picked = self.list.picked().cloned();
        let body = div()
            .flex()
            .h(px(460.))
            .child(
                div()
                    .id("files-rows")
                    .flex_1()
                    .min_w_0()
                    .overflow_y_scroll()
                    .py_2()
                    .flex()
                    .flex_col()
                    .track_scroll(&self.scroll)
                    .when(row_elements.is_empty(), |rows| {
                        rows.child(
                            div()
                                .px_4()
                                .py_3()
                                .text_color(colors.muted_text)
                                .child("No file path holds every word."),
                        )
                    })
                    .children(row_elements),
            )
            .child(Self::detail(&colors, picked.as_ref(), cx));
        modal_layer_for(&colors, "files-scrim", &self.presence)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| this.close(window, cx)),
            )
            .child(rise_for(
                overlay_panel(&colors)
                    .id("files-sheet")
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
                            .child(key_hint(
                                &colors,
                                &[
                                    KeyLabel::Icon(Icon::ArrowLeft),
                                    KeyLabel::Icon(Icon::ArrowRight),
                                ],
                                "fold",
                            ))
                            .child(key_hint(
                                &colors,
                                &[KeyLabel::Text("return")],
                                "first screen",
                            ))
                            .child(key_hint(&colors, &[KeyLabel::Text("1-9")], "that screen"))
                            .child(key_hint(&colors, &[KeyLabel::Text("?")], "search")),
                    ),
                "files-sheet-rise",
                &self.presence,
            ))
            .into_any_element()
    }
}
