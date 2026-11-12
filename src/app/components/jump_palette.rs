use std::time::Instant;

use gpui::prelude::FluentBuilder;
use gpui::{
    Context, EventEmitter, FocusHandle, FontWeight, InteractiveElement, IntoElement, KeyDownEvent,
    ParentElement, Render, SharedString, StatefulInteractiveElement, Styled, Window, div, px,
};

use crate::pieces::PieceId;
use crate::viewer::{
    JumpPlace, JumpTarget, PaletteAction, PaletteEntry, PaletteGroup, PaletteGroupKind, PieceLabel,
    Presence,
};

use super::icon::{Icon, sized_icon};
use super::key_cap::{CapGround, KeyLabel, chord_caps, key_hint};
use super::markdown_view::InlineMarkdown;
use super::motion::OVERLAY;
use super::overlay::{
    close_presence, modal_layer_for, overlay_panel, picker_footer, picker_row, rise_for,
};
use super::palette::Colors;
use super::text_box::TextBox;
use super::text_input::Lines;

/// Rows of the list on show at once, headings included.
const VISIBLE_ROWS: usize = 13;

pub enum JumpEvent {
    /// The list must be asked again for this text.
    QueryChanged(String),
    Picked(JumpTarget),
    Act(PaletteAction),
}

/// The cmd+k overlay: a query box over the places the reviewer can jump to and the actions the app
/// can take, under their headings. It knows nothing of the review; its owner answers `QueryChanged`
/// with new groups.
pub struct JumpPalette {
    presence: Presence,
    query: String,
    rows: Vec<ShownRow>,
    /// Of the entries, headings not counted.
    entry_count: usize,
    selected: usize,
    focus: FocusHandle,
    owner_focus: FocusHandle,
}

/// A row of the list with the text it shows, cut when the list changes rather than every frame.
enum ShownRow {
    Heading(PaletteGroupKind),
    Entry {
        index: usize,
        entry: Box<ShownEntry>,
    },
}

struct ShownEntry {
    entry: PaletteEntry,
    label: ShownLabel,
    detail: Option<SharedString>,
}

enum ShownLabel {
    Step(SharedString),
    Piece(PieceLabel),
    Action(&'static str),
}

impl ShownEntry {
    fn of(entry: PaletteEntry) -> Self {
        let (label, detail) = match &entry {
            PaletteEntry::Place(target) => (
                match &target.place {
                    JumpPlace::StepTop => ShownLabel::Step(target.label().into_owned().into()),
                    JumpPlace::File(path) => {
                        ShownLabel::Piece(PieceLabel::of(&PieceId::whole_file(path.clone())))
                    }
                    JumpPlace::Piece(id) => ShownLabel::Piece(PieceLabel::of(id)),
                },
                target.detail().map(|detail| detail.to_owned().into()),
            ),
            PaletteEntry::Action(action) => (ShownLabel::Action(action.label()), None),
        };
        Self {
            entry,
            label,
            detail,
        }
    }

    fn icon(&self) -> Icon {
        match &self.entry {
            PaletteEntry::Place(target) => match target.place {
                JumpPlace::StepTop => Icon::Milestone,
                JumpPlace::File(_) => Icon::FileText,
                JumpPlace::Piece(_) => Icon::Braces,
            },
            PaletteEntry::Action(action) => action_icon(*action),
        }
    }
}

fn action_icon(action: PaletteAction) -> Icon {
    match action {
        PaletteAction::NextUnreadStep => Icon::CircleDot,
        PaletteAction::MarkStepRead | PaletteAction::MarkStepUnread => Icon::Check,
        PaletteAction::WrapLines | PaletteAction::StopWrappingLines => Icon::WrapText,
        PaletteAction::HideWhitespace | PaletteAction::ShowWhitespace => Icon::Space,
        PaletteAction::AskAboutStep => Icon::Sparkles,
        PaletteAction::FinishReview => Icon::ListChecks,
        PaletteAction::OpenOnGitHub => Icon::ExternalLink,
        PaletteAction::SwitchPr => Icon::PullRequest,
        PaletteAction::Settings => Icon::Settings,
        PaletteAction::Shortcuts => Icon::Keyboard,
        PaletteAction::RestartApp => Icon::RotateCcw,
    }
}

impl EventEmitter<JumpEvent> for JumpPalette {}

impl JumpPalette {
    pub const KEY_CONTEXT: &str = "JumpInput";

    pub fn new(owner_focus: FocusHandle, cx: &mut Context<Self>) -> Self {
        Self {
            presence: Presence::lasting(OVERLAY),
            query: String::new(),
            rows: Vec::new(),
            entry_count: 0,
            selected: 0,
            focus: cx.focus_handle(),
            owner_focus,
        }
    }

    pub fn is_open(&self) -> bool {
        self.presence.is_open()
    }

    pub fn open(&mut self, groups: Vec<PaletteGroup>, window: &mut Window, cx: &mut Context<Self>) {
        self.presence.open(Instant::now());
        self.query.clear();
        self.set_groups(groups);
        window.focus(&self.focus, cx);
        cx.notify();
    }

    /// The palette as it is after typing `query`, for a picture.
    #[cfg(feature = "snapshots")]
    pub fn stage_query(&mut self, query: &str, groups: Vec<PaletteGroup>) {
        query.clone_into(&mut self.query);
        self.set_groups(groups);
    }

    pub fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.presence.is_open() {
            return;
        }
        close_presence(self, cx, |this| &mut this.presence);
        window.focus(&self.owner_focus, cx);
        cx.notify();
    }

    pub fn set_groups(&mut self, groups: Vec<PaletteGroup>) {
        self.rows.clear();
        let mut index = 0;
        for group in groups {
            self.rows.push(ShownRow::Heading(group.group));
            for entry in group.entries {
                self.rows.push(ShownRow::Entry {
                    index,
                    entry: Box::new(ShownEntry::of(entry)),
                });
                index += 1;
            }
        }
        self.entry_count = index;
        self.selected = 0;
    }

    fn pick(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let picked = self.rows.iter().find_map(|row| match row {
            ShownRow::Entry { index, entry } if *index == self.selected => {
                Some(entry.entry.clone())
            }
            _ => None,
        });
        let Some(picked) = picked else {
            return;
        };
        self.close(window, cx);
        cx.emit(match picked {
            PaletteEntry::Place(target) => JumpEvent::Picked(target),
            PaletteEntry::Action(action) => JumpEvent::Act(action),
        });
    }

    /// Up and down pick a target; the query box takes every other key.
    fn on_key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        match event.keystroke.key.as_str() {
            "down" => self.selected = (self.selected + 1).min(self.entry_count.saturating_sub(1)),
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

    fn row(&self, index: usize, shown: &ShownEntry, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = Colors::active(cx);
        let is_selected = index == self.selected;
        let label = match &shown.label {
            ShownLabel::Step(title) => div()
                .min_w_0()
                .truncate()
                .child(InlineMarkdown::new(format!("jump-label-{index}"), title))
                .into_any_element(),
            ShownLabel::Piece(label) => piece_label(&colors, label.clone()).into_any_element(),
            ShownLabel::Action(name) => div().min_w_0().truncate().child(*name).into_any_element(),
        };
        let detail = shown.detail.as_ref().map(|detail| {
            div()
                .flex_shrink(1.)
                .min_w(px(80.))
                .max_w(px(260.))
                .truncate()
                .text_xs()
                .text_color(colors.muted_text)
                .child(InlineMarkdown::new(format!("jump-detail-{index}"), detail))
        });
        let keys = match &shown.entry {
            PaletteEntry::Action(action) => action
                .command()
                .map(|command| chord_caps(&colors, command.first_chord(), CapGround::Page)),
            PaletteEntry::Place(_) => None,
        };
        picker_row(
            &colors,
            SharedString::from(format!("jump-{index}")),
            is_selected,
        )
        .h(px(32.))
        .on_click(cx.listener(move |this, _, window, cx| {
            this.selected = index;
            this.pick(window, cx);
        }))
        .child(sized_icon(
            shown.icon(),
            if is_selected {
                colors.accent
            } else {
                colors.muted_text
            },
            px(14.),
        ))
        .child(div().flex_1().min_w_0().overflow_hidden().child(label))
        .children(detail)
        .children(keys)
    }
}

fn heading_row(colors: &Colors, group: PaletteGroupKind) -> impl IntoElement {
    div()
        .px_4()
        .pt_2()
        .pb_1()
        .text_xs()
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(colors.muted_text)
        .child(group.title())
}

/// A file or symbol target as `folder/` dim, then the file name, then the symbol, so the end of a
/// long path is what stays readable.
fn piece_label(colors: &Colors, label: PieceLabel) -> impl IntoElement {
    let PieceLabel {
        folder,
        file_name: file,
        symbol,
        symbol_name,
        is_part: _,
    } = label;
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
        if self.presence.shown().is_none() {
            return div().into_any_element();
        }
        let selected_row = self
            .rows
            .iter()
            .position(|row| matches!(row, ShownRow::Entry { index, .. } if *index == self.selected))
            .unwrap_or(0);
        let first_shown = selected_row.saturating_sub(VISIBLE_ROWS - 1);
        let rows: Vec<_> = self
            .rows
            .iter()
            .skip(first_shown)
            .take(VISIBLE_ROWS)
            .map(|row| match row {
                ShownRow::Heading(group) => heading_row(&colors, *group).into_any_element(),
                ShownRow::Entry { index, entry } => self.row(*index, entry, cx).into_any_element(),
            })
            .collect();
        modal_layer_for(&colors, "jump-palette-scrim", &self.presence)
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|this, _, window, cx| this.close(window, cx)),
            )
            .child(rise_for(
                overlay_panel(&colors)
                    .id("jump-palette")
                    .key_context(Self::KEY_CONTEXT)
                    .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_key_down(cx.listener(Self::on_key_down))
                    .mt(px(80.))
                    .w(px(720.))
                    .h_auto()
                    .child(
                        div()
                            .px_4()
                            .py_3()
                            .flex()
                            .items_center()
                            .gap_3()
                            .text_base()
                            .border_b_1()
                            .border_color(colors.subtle_border)
                            .child(sized_icon(Icon::Search, colors.muted_text, px(16.)))
                            .child(div().flex_1().min_w_0().child(self.query_box(cx))),
                    )
                    .child(div().py_2().flex().flex_col().children(rows).when(
                        self.entry_count == 0,
                        |list| {
                            list.child(
                                div().px_4().py_3().text_color(colors.muted_text).child(
                                    "Nothing matches. Try part of a file, symbol or step name, or > for actions.",
                                ),
                            )
                        },
                    ))
                    .child(
                        picker_footer(&colors)
                            .child(key_hint(
                                &colors,
                                &[
                                    KeyLabel::Icon(Icon::ArrowUp),
                                    KeyLabel::Icon(Icon::ArrowDown),
                                ],
                                "move",
                            ))
                            .child(key_hint(&colors, &[KeyLabel::Icon(Icon::Enter)], "open"))
                            .child(key_hint(&colors, &[KeyLabel::Text("esc")], "close"))
                            .child(div().flex_1())
                            .child(key_hint(&colors, &[KeyLabel::Text(">")], "actions only")),
                    ),
                "jump-palette-rise",
                &self.presence,
            ))
            .into_any_element()
    }
}
