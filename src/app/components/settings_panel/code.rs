//! The Code page: the editor code opens in, more lines, the language servers and the clones.

use gpui::prelude::FluentBuilder;
use gpui::{
    Context, FontWeight, InteractiveElement, IntoElement, ParentElement, SharedString,
    StatefulInteractiveElement, Styled, div, px,
};

use super::super::button::{ButtonKind, ButtonSize, icon_button, icon_label_button};
use super::super::icon::{Icon, sized_icon};
use super::super::motion::pop_when_shown;
use super::super::palette::{CODE_FONT, CONTROL_RADIUS, Colors, INNER_RADIUS};
use super::super::segmented::{Segment, SegmentLayout, Segmented};
use super::{ClonePart, Editing, SettingsPanel, Slot, rising, setting_row};
use crate::ide::{IdeChoice, IdeTemplate};
use crate::navigation::ServerAnswer;

impl SettingsPanel {
    pub(super) fn ide(
        &self,
        editing: &Editing,
        colors: &Colors,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let current = editing.settings.ide();
        let is_custom = editing.ide.is_open;
        let preset = |id: &'static str, name: &'static str, glyph: Glyph, choice: IdeChoice| {
            let is_on = !is_custom && current == choice;
            ide_tile(colors, id, name, glyph, is_on).on_click(cx.listener(move |this, _, _, cx| {
                let choice = choice.clone();
                this.edit(cx, |editing| {
                    editing.ide.is_open = false;
                    editing.settings.set_ide(&choice);
                });
            }))
        };
        let custom = ide_tile(
            colors,
            "ide-custom",
            "Custom",
            Glyph::Icon(Icon::Terminal),
            is_custom,
        )
        .on_click(cx.listener(|this, _, _, cx| {
            this.edit(cx, |editing| {
                if !editing.ide.is_open {
                    editing.ide.is_open = true;
                    editing.ide.text = editing.settings.ide_custom_text().to_owned();
                    editing
                        .settings
                        .set_ide(&IdeChoice::Custom(IdeTemplate::new(
                            editing.ide.text.clone(),
                        )));
                }
            });
        }));
        let tiles = div()
            .flex()
            .flex_wrap()
            .gap_3()
            .child(preset(
                "ide-jetbrains",
                "JetBrains",
                Glyph::Monogram("JB"),
                IdeChoice::JetBrains,
            ))
            .child(preset(
                "ide-vscode",
                "VS Code",
                Glyph::Monogram("VS"),
                IdeChoice::VsCode,
            ))
            .child(preset(
                "ide-zed",
                "Zed",
                Glyph::Monogram("Z"),
                IdeChoice::Zed,
            ))
            .child(preset(
                "ide-none",
                "None",
                Glyph::Icon(Icon::Globe),
                IdeChoice::None,
            ))
            .child(custom);
        let detail = if is_custom {
            editing.ide.focus.as_ref().map(|focus| {
                rising(
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(
                            self.text_box(
                                "ide-template",
                                focus,
                                &editing.ide.text,
                                editing.has_problem_under(Slot::Ide),
                                cx,
                                |editing, text| {
                                    editing
                                        .settings
                                        .set_ide(&IdeChoice::Custom(IdeTemplate::new(text.clone())));
                                    editing.ide.text = text;
                                },
                            )
                            .font_family(*CODE_FONT),
                        )
                        .child(div().text_xs().text_color(colors.muted_text).child(
                            "A link or a shell command. {path} and {line} become the file and the line.",
                        )),
                    "ide-custom-rise",
                )
                .into_any_element()
            })
        } else {
            Some(match current.shown_template() {
                Some(template) => note_row(colors, Icon::ExternalLink)
                    .child(Self::shown_text(colors, template))
                    .into_any_element(),
                None => note_row(colors, Icon::Globe)
                    .child("Code links open the file on its code host.")
                    .into_any_element(),
            })
        };
        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(tiles)
            .children(detail)
            .child(Self::problem_lines(editing, Slot::Ide, colors))
    }

    /// The repos the reviewer was asked about, each with its answer.
    pub(super) fn language_servers(
        editing: &Editing,
        colors: &Colors,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let rows = editing.settings.language_servers();
        if rows.is_empty() {
            return div()
                .text_sm()
                .text_color(colors.muted_text)
                .child("No repo yet. The Definition card asks on the first hover in a repo.")
                .into_any_element();
        }
        div()
            .flex()
            .flex_col()
            .gap_3()
            .children(rows.iter().enumerate().map(|(index, row)| {
                let segments = [ServerAnswer::Yes, ServerAnswer::No]
                    .into_iter()
                    .map(|answer| {
                        Segment::new(
                            if answer.is_yes() { "yes" } else { "no" },
                            if answer.is_yes() { "Use" } else { "Do not use" },
                            cx.listener(move |this, _, _, cx| {
                                this.edit(cx, |editing| {
                                    editing.settings.change_language_servers(index, answer);
                                });
                            }),
                        )
                    })
                    .collect();
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_x_6()
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .font_family(*CODE_FONT)
                            .text_sm()
                            .child(SharedString::from(row.codebase.to_string())),
                    )
                    .child(
                        Segmented::new(
                            SharedString::from(format!("language-servers-{index}")),
                            SegmentLayout::Row {
                                segment_width: px(110.),
                            },
                            segments,
                        )
                        .picked(Some(usize::from(!row.answer.is_yes()))),
                    )
            }))
            .into_any_element()
    }

    pub(super) fn more_lines(
        &self,
        editing: &Editing,
        colors: &Colors,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let focus = editing.more_lines.focus.clone();
        let note = div()
            .text_xs()
            .text_color(colors.muted_text)
            .child(editing.default_more_lines_note.clone());
        let input = focus.map(|focus| {
            div().w(px(120.)).child(self.text_box(
                "more-lines-count",
                &focus,
                &editing.more_lines.text,
                editing.has_problem_under(Slot::MoreLines),
                cx,
                |editing, text| {
                    editing.settings.set_more_lines_text(&text);
                    editing.more_lines.text = text;
                },
            ))
        });
        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(setting_row("Lines per step", note, div().children(input)))
            .child(Self::problem_lines(editing, Slot::MoreLines, colors))
    }

    pub(super) fn clones(
        &self,
        editing: &Editing,
        colors: &Colors,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let add_clone = |kind: ButtonKind, size: ButtonSize| {
            icon_label_button(colors, "clone-add", kind, size, Icon::Plus, "Add clone").on_click(
                cx.listener(|this, _, _, cx| {
                    this.edit(cx, |editing| {
                        editing.settings.add_clone();
                    });
                }),
            )
        };
        if editing.settings.clones().is_empty() {
            return div()
                .py_4()
                .flex()
                .flex_col()
                .items_center()
                .gap_3()
                .rounded(px(CONTROL_RADIUS))
                .border_1()
                .border_dashed()
                .border_color(colors.border)
                .child(
                    div()
                        .size(px(40.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_full()
                        .bg(colors.inset)
                        .child(sized_icon(Icon::FolderGit, colors.muted_text, px(20.))),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap_1()
                        .child(div().font_weight(FontWeight::MEDIUM).child("No clones yet"))
                        .child(
                            div()
                                .text_sm()
                                .text_color(colors.muted_text)
                                .child("Add one so code links open files from your own checkout."),
                        ),
                )
                .child(add_clone(ButtonKind::Tinted, ButtonSize::Regular))
                .into_any_element();
        }
        let rows = editing
            .settings
            .clones()
            .iter()
            .enumerate()
            .filter_map(|(row, clone)| {
                let focus = self.clone_focus.get(row)?;
                let label = editing.folder_labels.get(row)?.clone();
                let has_folder = !clone.folder.as_os_str().is_empty();
                let is_folder_wrong = editing.has_clone_problem(row, ClonePart::Folder);
                let folder_color = if is_folder_wrong {
                    colors.danger
                } else if has_folder {
                    colors.text
                } else {
                    colors.muted_text
                };
                Some(
                    div()
                        .px_3()
                        .py_2p5()
                        .flex()
                        .flex_col()
                        .gap_1p5()
                        .when(row > 0, |line| {
                            line.border_t_1().border_color(colors.subtle_border)
                        })
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_3()
                                .child(
                                    self.text_box(
                                        SharedString::from(format!("clone-repo-{row}")),
                                        focus,
                                        &clone.repo,
                                        editing.has_clone_problem(row, ClonePart::Repo),
                                        cx,
                                        move |editing, text| {
                                            editing.settings.set_clone_repo(row, text);
                                        },
                                    )
                                    .w(px(210.))
                                    .flex_none(),
                                )
                                .child(sized_icon(Icon::ArrowRight, colors.muted_text, px(14.)))
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .flex()
                                        .items_center()
                                        .gap_2()
                                        .child(sized_icon(Icon::FolderOpen, folder_color, px(14.)))
                                        .child(
                                            div()
                                                .flex_1()
                                                .min_w_0()
                                                .overflow_hidden()
                                                .whitespace_nowrap()
                                                .text_ellipsis_middle()
                                                .text_xs()
                                                .when(has_folder, |path| {
                                                    path.font_family(*CODE_FONT)
                                                })
                                                .text_color(folder_color)
                                                .child(label),
                                        ),
                                )
                                .child(
                                    icon_button(
                                        colors,
                                        SharedString::from(format!("clone-folder-{row}")),
                                        ButtonKind::Secondary,
                                        ButtonSize::Regular,
                                        Icon::FolderOpen,
                                        "Choose folder",
                                    )
                                    .on_click(cx.listener(
                                        move |this, _, _, cx| {
                                            this.pick_clone_folder(row, cx);
                                        },
                                    )),
                                )
                                .child(
                                    icon_button(
                                        colors,
                                        SharedString::from(format!("clone-remove-{row}")),
                                        ButtonKind::Danger,
                                        ButtonSize::Regular,
                                        Icon::Trash,
                                        "Remove clone",
                                    )
                                    .on_click(cx.listener(
                                        move |this, _, _, cx| {
                                            this.edit(cx, |editing| {
                                                editing.settings.remove_clone(row);
                                            });
                                        },
                                    )),
                                ),
                        )
                        .child(Self::problem_lines(editing, Slot::Clone(row), colors)),
                )
            });
        div()
            .flex()
            .flex_col()
            .gap_3()
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
            .child(
                div()
                    .flex()
                    .child(add_clone(ButtonKind::Ghost, ButtonSize::Small)),
            )
            .into_any_element()
    }
}

/// A quiet line that says what a choice does.
fn note_row(colors: &Colors, icon: Icon) -> gpui::Div {
    div()
        .flex()
        .items_center()
        .gap_2()
        .px_3()
        .py_2()
        .rounded(px(CONTROL_RADIUS))
        .bg(colors.inset)
        .text_sm()
        .text_color(colors.muted_text)
        .child(sized_icon(icon, colors.muted_text, px(14.)))
}

/// What an IDE tile shows above its name: a neutral monogram, or a drawn icon.
#[derive(Debug, Clone, Copy)]
enum Glyph {
    Monogram(&'static str),
    Icon(Icon),
}

fn ide_tile(
    colors: &Colors,
    id: &'static str,
    name: &'static str,
    glyph: Glyph,
    is_on: bool,
) -> gpui::Stateful<gpui::Div> {
    let ink = if is_on { colors.accent } else { colors.text };
    let mark = div()
        .size(px(30.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(INNER_RADIUS))
        .border_1()
        .text_xs()
        .font_weight(FontWeight::BOLD)
        .text_color(ink)
        .map(|mark| {
            if is_on {
                mark.bg(colors.icon_tile.diagonal())
                    .border_color(colors.current_border)
            } else {
                mark.bg(colors.inset).border_color(colors.subtle_border)
            }
        })
        .child(match glyph {
            Glyph::Monogram(letters) => letters.into_any_element(),
            Glyph::Icon(icon) => sized_icon(icon, ink, px(16.)).into_any_element(),
        });
    div()
        .id(id)
        .role(gpui::Role::Button)
        .aria_label(name)
        .aria_toggled(if is_on {
            gpui::Toggled::True
        } else {
            gpui::Toggled::False
        })
        .relative()
        .flex_1()
        .min_w(px(120.))
        .flex()
        .flex_col()
        .gap_2p5()
        .p_3()
        .rounded(px(CONTROL_RADIUS))
        .border_1()
        .cursor_pointer()
        .map(|tile| {
            if is_on {
                tile.bg(colors.current).border_color(colors.accent_border)
            } else {
                tile.bg(colors.raised_fill())
                    .border_color(colors.border)
                    .shadow(colors.control_shadow())
                    .hover(|tile| tile.border_color(colors.accent_border))
                    .active(|tile| tile.bg(colors.pressed).shadow_none())
            }
        })
        .child(mark)
        .child(
            div()
                .text_sm()
                .font_weight(FontWeight::MEDIUM)
                .text_color(ink)
                .child(name),
        )
        .child(pop_when_shown(
            sized_icon(Icon::CircleCheck, colors.accent, px(16.))
                .absolute()
                .top(px(10.))
                .right(px(10.)),
            "tile-check",
            is_on,
        ))
}
