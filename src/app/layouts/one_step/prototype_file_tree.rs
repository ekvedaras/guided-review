//! PROTOTYPE for #137: three ways to show the changed files as a tree, switched with the pill at the
//! bottom of the window (`shift-t` or its arrows); `t` opens and closes the tree. Throwaway: it lives
//! on the `prototype-137-file-tree` branch only and never goes to main.
//!
//! A: a panel on the right that stays open, the page gets narrower; each file lists its steps as chips.
//! B: a popup over the page, like the comments list; the tree on the left, the steps of the picked file
//!    on the right.
//! C: a Files tab in the sidebar, beside Steps; the same rows as a basic tour's tree.

use std::collections::BTreeSet;

use gpui::prelude::FluentBuilder;
use gpui::{
    AnyElement, Context, FocusHandle, FontWeight, InteractiveElement, IntoElement, KeyDownEvent,
    MouseButton, ParentElement, SharedString, StatefulInteractiveElement, Styled, Window, actions,
    div, px,
};

use super::OneStepLayout;
use crate::app::components::icon::{Icon, sized_icon};
use crate::app::components::key_cap::{KeyLabel, key_hint};
use crate::app::components::overlay::{modal_layer, overlay_panel, picker_footer};
use crate::app::components::palette::{CODE_FONT, Colors};
use crate::app::components::sidebar_row::{RowMark, RowPlace, row_label, sidebar_row};
use crate::viewer::{
    FolderPath, JumpTarget, ProtoFile, ProtoFileTree, ProtoRowKind, ProtoStep, Screen,
};

actions!(prototype_file_tree, [ProtoToggleFileTree, ProtoNextVariant]);

pub const PROTO_SHEET_CONTEXT: &str = "ProtoFileTreeSheet";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProtoVariant {
    SidePanel,
    Popup,
    SidebarTab,
}

impl ProtoVariant {
    const ALL: [Self; 3] = [Self::SidePanel, Self::Popup, Self::SidebarTab];

    fn key(self) -> &'static str {
        match self {
            Self::SidePanel => "A",
            Self::Popup => "B",
            Self::SidebarTab => "C",
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::SidePanel => "Side panel",
            Self::Popup => "Popup",
            Self::SidebarTab => "Sidebar tab",
        }
    }

    fn step(self, by: isize) -> Self {
        let at = Self::ALL.iter().position(|v| *v == self).unwrap_or(0) as isize;
        let len = Self::ALL.len() as isize;
        Self::ALL[((at + by).rem_euclid(len)) as usize]
    }

    fn from_env() -> Self {
        match std::env::var("GR_PROTO_TREE").ok().as_deref() {
            Some("b" | "B") => Self::Popup,
            Some("c" | "C") => Self::SidebarTab,
            _ => Self::SidePanel,
        }
    }
}

pub struct ProtoTree {
    pub variant: ProtoVariant,
    pub is_open: bool,
    folded: BTreeSet<FolderPath>,
    /// The popup's cursor, an index into the tree's rows.
    cursor: usize,
    focus: FocusHandle,
}

impl ProtoTree {
    pub fn new(focus: FocusHandle) -> Self {
        Self {
            variant: ProtoVariant::from_env(),
            is_open: false,
            folded: BTreeSet::new(),
            cursor: 0,
            focus,
        }
    }

    pub fn shows_side_panel(&self) -> bool {
        self.is_open && self.variant == ProtoVariant::SidePanel
    }

    pub fn shows_popup(&self) -> bool {
        self.is_open && self.variant == ProtoVariant::Popup
    }

    pub fn shows_sidebar_files(&self) -> bool {
        self.is_open && self.variant == ProtoVariant::SidebarTab
    }
}

fn stats(colors: &Colors, added: usize, removed: usize) -> impl IntoElement {
    div()
        .flex_none()
        .flex()
        .gap_1()
        .font_family(*CODE_FONT)
        .text_xs()
        .child(div().text_color(colors.success).child(format!("+{added}")))
        .child(div().text_color(colors.danger).child(format!("−{removed}")))
}

fn tick(colors: &Colors, is_read: bool) -> impl IntoElement {
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

impl OneStepLayout {
    fn proto_tree_data(&self, cx: &Context<Self>) -> ProtoFileTree {
        self.session
            .read(cx)
            .review()
            .prototype_file_tree(&self.proto_tree.folded)
    }

    pub(super) fn proto_toggle(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.proto_tree.is_open = !self.proto_tree.is_open;
        if self.proto_tree.shows_popup() {
            window.focus(&self.proto_tree.focus, cx);
        } else {
            window.focus(&self.focus, cx);
        }
        cx.notify();
    }

    pub(super) fn proto_next_variant(
        &mut self,
        by: isize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.proto_tree.variant = self.proto_tree.variant.step(by);
        if self.proto_tree.shows_popup() {
            window.focus(&self.proto_tree.focus, cx);
        } else {
            window.focus(&self.focus, cx);
        }
        cx.notify();
    }

    fn proto_toggle_folder(&mut self, folder: &FolderPath, cx: &mut Context<Self>) {
        if !self.proto_tree.folded.remove(folder) {
            self.proto_tree.folded.insert(folder.clone());
        }
        cx.notify();
    }

    /// To the step (the first one that shows the file when none is named), scrolled to the file.
    fn proto_go(&mut self, file: &ProtoFile, step: Option<&ProtoStep>, cx: &mut Context<Self>) {
        let screen = match step.or_else(|| file.steps.first()) {
            Some(step) => Screen::Step(step.id.clone()),
            None if file.in_safety_net => Screen::SafetyNet,
            None => Screen::File(file.path.clone()),
        };
        self.jump_to(&JumpTarget::prototype_file(screen, file.path.clone()), cx);
        cx.notify();
    }

    fn proto_step_chips(
        &self,
        colors: &Colors,
        file: &ProtoFile,
        id: &str,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        div()
            .flex_none()
            .flex()
            .gap_1()
            .children(file.steps.iter().map(|step| {
                let (file, step) = (file.clone(), step.clone());
                div()
                    .id(SharedString::from(format!(
                        "{id}-chip-{}-{}",
                        file.path, step.number
                    )))
                    .px_1p5()
                    .rounded(px(4.))
                    .border_1()
                    .text_xs()
                    .cursor_pointer()
                    .map(|chip| {
                        if step.is_current {
                            chip.bg(colors.accent)
                                .text_color(colors.on_accent_fill)
                                .border_color(colors.accent)
                        } else if step.is_read {
                            chip.text_color(colors.muted_text)
                                .border_color(colors.subtle_border)
                        } else {
                            chip.text_color(colors.text).border_color(colors.border)
                        }
                    })
                    .hover(|chip| chip.bg(colors.hover))
                    .child(step.number.to_string())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        this.proto_go(&file, Some(&step), cx);
                    }))
            }))
            .when(file.in_safety_net, |chips| {
                chips.child(
                    div()
                        .px_1p5()
                        .rounded(px(4.))
                        .border_1()
                        .border_color(colors.warning_border)
                        .text_xs()
                        .text_color(colors.warning)
                        .child("net"),
                )
            })
    }

    // ---- A: side panel ----

    pub(super) fn proto_side_panel(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.proto_tree.shows_side_panel() {
            return None;
        }
        let colors = Colors::active(cx);
        let tree = self.proto_tree_data(cx);
        let rows = tree.rows.iter().enumerate().map(|(index, row)| {
            let base = div()
                .id(("proto-a-row", index))
                .mx_2()
                .pl(px(8. + row.depth as f32 * 14.))
                .pr_2()
                .py_1()
                .flex()
                .items_center()
                .gap_2()
                .rounded(px(6.))
                .cursor_pointer()
                .hover(|row| row.bg(colors.hover));
            match &row.kind {
                ProtoRowKind::Folder {
                    path,
                    label,
                    is_folded,
                    added,
                    removed,
                    is_read,
                } => {
                    let folder = path.clone();
                    base.text_color(if *is_read {
                        colors.muted_text
                    } else {
                        colors.sidebar_text
                    })
                    .on_click(
                        cx.listener(move |this, _, _, cx| this.proto_toggle_folder(&folder, cx)),
                    )
                    .child(sized_icon(
                        if *is_folded {
                            Icon::ChevronRight
                        } else {
                            Icon::ChevronDown
                        },
                        colors.muted_text,
                        px(12.),
                    ))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .font_weight(FontWeight::MEDIUM)
                            .child(label.clone()),
                    )
                    .child(stats(&colors, *added, *removed))
                    .into_any_element()
                }
                ProtoRowKind::File(file) => {
                    let go = file.clone();
                    base.when(file.on_screen, |row| row.bg(colors.current))
                        .text_color(if file.is_read {
                            colors.muted_text
                        } else {
                            colors.text
                        })
                        .on_click(cx.listener(move |this, _, _, cx| this.proto_go(&go, None, cx)))
                        .child(tick(&colors, file.is_read))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .child(file.label.clone()),
                        )
                        .child(stats(&colors, file.added, file.removed))
                        .child(self.proto_step_chips(&colors, file, "proto-a", cx))
                        .into_any_element()
                }
            }
        });
        Some(
            div()
                .w(px(380.))
                .flex_none()
                .h_full()
                .flex()
                .flex_col()
                .bg(colors.background)
                .border_l_1()
                .border_color(colors.subtle_border)
                .text_sm()
                .child(
                    div()
                        .px_4()
                        .pt(px(44.))
                        .pb_3()
                        .flex()
                        .items_center()
                        .gap_2()
                        .border_b_1()
                        .border_color(colors.subtle_border)
                        .child(sized_icon(Icon::FolderOpen, colors.accent, px(16.)))
                        .child(div().flex_1().font_weight(FontWeight::SEMIBOLD).child(format!("Changed files · {}", tree.file_count)))
                        .child(stats(&colors, tree.added, tree.removed)),
                )
                .child(
                    div()
                        .px_4()
                        .py_2()
                        .text_xs()
                        .text_color(colors.muted_text)
                        .child(if tree.is_guided {
                            "Chips are the steps that show the file. Click a row for its first step, a chip for that step."
                        } else {
                            "Click a file to open it."
                        }),
                )
                .child(
                    div()
                        .id("proto-a-rows")
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scroll()
                        .py_1()
                        .flex()
                        .flex_col()
                        .gap_px()
                        .children(rows),
                )
                .into_any_element(),
        )
    }

    // ---- B: popup ----

    fn proto_popup_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.stop_propagation();
        let tree = self.proto_tree_data(cx);
        let last = tree.rows.len().saturating_sub(1);
        let cursor = self.proto_tree.cursor.min(last);
        let under = tree.rows.get(cursor).map(|row| row.kind.clone());
        match event.keystroke.key.as_str() {
            "escape" | "t" => self.proto_toggle(window, cx),
            "down" | "j" => self.proto_tree.cursor = if cursor >= last { 0 } else { cursor + 1 },
            "up" | "k" => self.proto_tree.cursor = if cursor == 0 { last } else { cursor - 1 },
            "left" | "right" => {
                if let Some(ProtoRowKind::Folder {
                    path, is_folded, ..
                }) = under
                {
                    let wants_fold = event.keystroke.key == "left";
                    if wants_fold != is_folded {
                        self.proto_toggle_folder(&path, cx);
                    }
                }
            }
            "enter" => match under {
                Some(ProtoRowKind::Folder { path, .. }) => self.proto_toggle_folder(&path, cx),
                Some(ProtoRowKind::File(file)) => {
                    self.proto_toggle(window, cx);
                    self.proto_go(&file, None, cx);
                }
                None => {}
            },
            digit if digit.len() == 1 && digit.chars().all(|c| c.is_ascii_digit()) => {
                if let Some(ProtoRowKind::File(file)) = under {
                    let nth: usize = digit.parse().unwrap_or(0);
                    if let Some(step) = nth.checked_sub(1).and_then(|n| file.steps.get(n)).cloned()
                    {
                        self.proto_toggle(window, cx);
                        self.proto_go(&file, Some(&step), cx);
                    }
                }
            }
            _ => {}
        }
        cx.notify();
    }

    pub(super) fn proto_popup(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.proto_tree.shows_popup() {
            return None;
        }
        let colors = Colors::active(cx);
        let tree = self.proto_tree_data(cx);
        let cursor = self
            .proto_tree
            .cursor
            .min(tree.rows.len().saturating_sub(1));
        let rows = tree.rows.iter().enumerate().map(|(index, row)| {
            let is_cursor = index == cursor;
            let base = div()
                .id(("proto-b-row", index))
                .mx_2()
                .pl(px(8. + row.depth as f32 * 14.))
                .pr_2()
                .py_1()
                .flex()
                .items_center()
                .gap_2()
                .rounded(px(6.))
                .border_1()
                .cursor_pointer()
                .map(|row| {
                    if is_cursor {
                        row.bg(colors.current).border_color(colors.current_border)
                    } else {
                        row.border_color(gpui::transparent_black())
                            .hover(|row| row.bg(colors.hover))
                    }
                });
            match &row.kind {
                ProtoRowKind::Folder {
                    path,
                    label,
                    is_folded,
                    added,
                    removed,
                    ..
                } => {
                    let folder = path.clone();
                    base.text_color(colors.sidebar_text)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.proto_tree.cursor = index;
                            this.proto_toggle_folder(&folder, cx);
                        }))
                        .child(sized_icon(
                            if *is_folded {
                                Icon::ChevronRight
                            } else {
                                Icon::ChevronDown
                            },
                            colors.muted_text,
                            px(12.),
                        ))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .font_weight(FontWeight::MEDIUM)
                                .child(label.clone()),
                        )
                        .child(stats(&colors, *added, *removed))
                        .into_any_element()
                }
                ProtoRowKind::File(file) => base
                    .text_color(if file.is_read {
                        colors.muted_text
                    } else {
                        colors.text
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.proto_tree.cursor = index;
                        cx.notify();
                    }))
                    .child(tick(&colors, file.is_read))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .child(file.label.clone()),
                    )
                    .when(file.steps.len() > 1, |row| {
                        row.child(
                            div()
                                .text_xs()
                                .text_color(colors.muted_text)
                                .child(format!("{} steps", file.steps.len())),
                        )
                    })
                    .child(stats(&colors, file.added, file.removed))
                    .into_any_element(),
            }
        });
        let picked = match tree.rows.get(cursor).map(|row| &row.kind) {
            Some(ProtoRowKind::File(file)) => Some(file.clone()),
            _ => None,
        };
        let detail = div()
            .w(px(380.))
            .flex_none()
            .border_l_1()
            .border_color(colors.subtle_border)
            .p_4()
            .flex()
            .flex_col()
            .gap_2()
            .map(|detail| match &picked {
                None => detail.child(
                    div()
                        .text_color(colors.muted_text)
                        .child("Pick a file to see the steps that show it."),
                ),
                Some(file) => detail
                    .child(
                        div()
                            .font_family(*CODE_FONT)
                            .text_xs()
                            .text_color(colors.muted_text)
                            .child(file.path.to_string()),
                    )
                    .child(div().pt_2().font_weight(FontWeight::SEMIBOLD).child(
                        match file.steps.len() {
                            0 if file.in_safety_net => "Only the Safety net shows it".to_owned(),
                            0 => "No step shows it".to_owned(),
                            1 => "Shown in 1 step".to_owned(),
                            n => format!("Shown in {n} steps"),
                        },
                    ))
                    .children(file.steps.iter().enumerate().map(|(nth, step)| {
                        let (go_file, go_step) = (file.clone(), step.clone());
                        div()
                            .id(SharedString::from(format!("proto-b-step-{}", step.number)))
                            .px_2()
                            .py_1p5()
                            .flex()
                            .items_center()
                            .gap_2()
                            .rounded(px(6.))
                            .cursor_pointer()
                            .hover(|row| row.bg(colors.hover))
                            .when(step.is_current, |row| row.bg(colors.current))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.proto_toggle(window, cx);
                                this.proto_go(&go_file, Some(&go_step), cx);
                            }))
                            .child(tick(&colors, step.is_read))
                            .child(
                                div()
                                    .flex_none()
                                    .text_xs()
                                    .text_color(colors.muted_text)
                                    .child(format!("{}", nth + 1)),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .child(format!("Step {} · {}", step.number, step.title)),
                            )
                    }))
                    .when(file.in_safety_net, |detail| {
                        detail.child(
                            div()
                                .px_2()
                                .py_1p5()
                                .text_color(colors.warning)
                                .child("Safety net"),
                        )
                    }),
            });
        Some(
            modal_layer(&colors, "proto-b-scrim")
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, window, cx| this.proto_toggle(window, cx)),
                )
                .child(
                    overlay_panel(&colors)
                        .id("proto-b-sheet")
                        .key_context(PROTO_SHEET_CONTEXT)
                        .track_focus(&self.proto_tree.focus)
                        .on_key_down(cx.listener(Self::proto_popup_key))
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .mt(px(80.))
                        .w(px(1000.))
                        .child(
                            div()
                                .px_5()
                                .pt_4()
                                .pb_3()
                                .flex()
                                .items_center()
                                .gap_3()
                                .border_b_1()
                                .border_color(colors.subtle_border)
                                .child(sized_icon(Icon::FolderOpen, colors.accent, px(18.)))
                                .child(
                                    div()
                                        .flex_1()
                                        .text_base()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .child(format!("Changed files ({})", tree.file_count)),
                                )
                                .child(stats(&colors, tree.added, tree.removed)),
                        )
                        .child(
                            div()
                                .flex()
                                .h(px(460.))
                                .child(
                                    div()
                                        .id("proto-b-rows")
                                        .flex_1()
                                        .min_w_0()
                                        .overflow_y_scroll()
                                        .py_2()
                                        .flex()
                                        .flex_col()
                                        .gap_px()
                                        .children(rows),
                                )
                                .child(detail),
                        )
                        .child(
                            picker_footer(&colors)
                                .child(key_hint(
                                    &colors,
                                    &[KeyLabel::Text("esc"), KeyLabel::Text("t")],
                                    "close",
                                ))
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
                                .child(key_hint(&colors, &[KeyLabel::Text("return")], "first step"))
                                .child(key_hint(&colors, &[KeyLabel::Text("1-9")], "that step")),
                        ),
                )
                .into_any_element(),
        )
    }

    // ---- C: sidebar tab ----

    pub(super) fn proto_sidebar_tabs(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = Colors::active(cx);
        let is_files = self.proto_tree.shows_sidebar_files();
        let tab = |id: &'static str, label: &'static str, is_on: bool| {
            div()
                .id(id)
                .flex_1()
                .py_1()
                .flex()
                .justify_center()
                .rounded(px(6.))
                .cursor_pointer()
                .text_xs()
                .font_weight(FontWeight::MEDIUM)
                .map(|tab| {
                    if is_on {
                        tab.bg(colors.current)
                            .text_color(colors.text)
                            .border_1()
                            .border_color(colors.current_border)
                    } else {
                        tab.text_color(colors.muted_text)
                            .hover(|tab| tab.bg(colors.hover))
                    }
                })
                .child(label)
        };
        div()
            .mx_4()
            .mb_1()
            .p_0p5()
            .flex()
            .gap_1()
            .rounded(px(8.))
            .bg(colors.inset)
            .child(
                tab("proto-c-steps", "Steps", !is_files).on_click(cx.listener(
                    |this, _, window, cx| {
                        if this.proto_tree.is_open {
                            this.proto_toggle(window, cx);
                        }
                    },
                )),
            )
            .child(
                tab("proto-c-files", "Files", is_files).on_click(cx.listener(
                    |this, _, window, cx| {
                        if !this.proto_tree.is_open {
                            this.proto_toggle(window, cx);
                        }
                    },
                )),
            )
            .into_any_element()
    }

    pub(super) fn proto_sidebar_files(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let colors = Colors::active(cx);
        let tree = self.proto_tree_data(cx);
        tree.rows
            .iter()
            .enumerate()
            .map(|(index, row)| match &row.kind {
                ProtoRowKind::Folder {
                    path,
                    label,
                    is_folded,
                    is_read,
                    ..
                } => {
                    let folder = path.clone();
                    sidebar_row(
                        &colors,
                        ("proto-c-row", index),
                        row.depth,
                        RowPlace::Apart,
                        if *is_read {
                            RowMark::Read
                        } else {
                            RowMark::Page
                        },
                        None,
                    )
                    .text_color(colors.muted_text)
                    .on_click(
                        cx.listener(move |this, _, _, cx| this.proto_toggle_folder(&folder, cx)),
                    )
                    .child(
                        div()
                            .flex_none()
                            .h(px(22.))
                            .flex()
                            .items_center()
                            .child(sized_icon(
                                if *is_folded {
                                    Icon::ChevronRight
                                } else {
                                    Icon::ChevronDown
                                },
                                colors.muted_text,
                                px(12.),
                            )),
                    )
                    .child(row_label(SharedString::from(label.clone())))
                    .into_any_element()
                }
                ProtoRowKind::File(file) => {
                    let go = file.clone();
                    let steps = file
                        .steps
                        .iter()
                        .map(|step| step.number.to_string())
                        .collect::<Vec<_>>()
                        .join(" · ");
                    sidebar_row(
                        &colors,
                        ("proto-c-row", index),
                        row.depth,
                        if file.on_screen {
                            RowPlace::Current
                        } else {
                            RowPlace::Apart
                        },
                        if file.is_read {
                            RowMark::Read
                        } else {
                            RowMark::Unread
                        },
                        None,
                    )
                    .on_click(cx.listener(move |this, _, _, cx| this.proto_go(&go, None, cx)))
                    .child(row_label(SharedString::from(file.label.clone())))
                    .child(
                        div()
                            .flex_none()
                            .text_xs()
                            .text_color(colors.muted_text)
                            .child(if file.in_safety_net && steps.is_empty() {
                                "net".to_owned()
                            } else {
                                steps
                            }),
                    )
                    .into_any_element()
                }
            })
            .collect()
    }

    #[cfg(feature = "snapshots")]
    pub(super) fn proto_stage(
        &mut self,
        variant: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.change_review(cx, |review| {
            review.go_to_summary();
            review.go_forward();
            review.go_forward();
        });
        self.proto_tree.variant = ProtoVariant::ALL[variant.min(2)];
        self.proto_tree.is_open = false;
        let tree = self.proto_tree_data(cx);
        let file_at = |wanted: &dyn Fn(&ProtoFile) -> bool| {
            tree.rows
                .iter()
                .position(|row| matches!(&row.kind, ProtoRowKind::File(file) if wanted(file)))
        };
        self.proto_tree.cursor = file_at(&|file| file.steps.len() > 1)
            .or_else(|| file_at(&|file| file.on_screen))
            .or_else(|| file_at(&|_| true))
            .unwrap_or(0);
        self.proto_toggle(window, cx);
    }

    #[cfg(feature = "snapshots")]
    pub(super) fn proto_unstage(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.proto_tree.is_open {
            self.proto_toggle(window, cx);
        }
        self.proto_tree.variant = ProtoVariant::SidePanel;
    }

    // ---- the switcher pill ----

    pub(super) fn proto_switcher(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = Colors::active(cx);
        let variant = self.proto_tree.variant;
        let arrow = |id: &'static str, icon: Icon| {
            div()
                .id(id)
                .px_2()
                .py_1()
                .rounded_full()
                .cursor_pointer()
                .hover(|arrow| arrow.bg(gpui::rgba(0xffffff22)))
                .child(sized_icon(icon, gpui::white().into(), px(14.)))
        };
        div()
            .absolute()
            .bottom(px(16.))
            .left_0()
            .right_0()
            .flex()
            .justify_center()
            .child(
                div()
                    .id("proto-switcher")
                    .occlude()
                    .px_2()
                    .py_1()
                    .flex()
                    .items_center()
                    .gap_2()
                    .rounded_full()
                    .bg(gpui::rgba(0x111111ee))
                    .text_color(gpui::white())
                    .text_sm()
                    .shadow(colors.overlay_shadow())
                    .child(arrow("proto-prev", Icon::ArrowLeft).on_click(
                        cx.listener(|this, _, window, cx| this.proto_next_variant(-1, window, cx)),
                    ))
                    .child(div().font_weight(FontWeight::SEMIBOLD).child(format!(
                        "PROTOTYPE {} · {}",
                        variant.key(),
                        variant.name()
                    )))
                    .child(
                        div()
                            .text_xs()
                            .opacity(0.7)
                            .child(if self.proto_tree.is_open {
                                "t closes · shift-t next"
                            } else {
                                "t opens · shift-t next"
                            }),
                    )
                    .child(arrow("proto-next", Icon::ArrowRight).on_click(
                        cx.listener(|this, _, window, cx| this.proto_next_variant(1, window, cx)),
                    )),
            )
            .into_any_element()
    }
}
