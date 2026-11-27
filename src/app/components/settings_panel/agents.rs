//! The Agents page: the summary and review instructions.

use gpui::prelude::FluentBuilder;
use gpui::{
    Context, IntoElement, ParentElement, SharedString, StatefulInteractiveElement, Styled, div, px,
};

use super::super::button::{ButtonKind, ButtonSize, icon_label_button, toggle_button};
use super::super::chip::chip;
use super::super::icon::{Icon, sized_icon};
use super::super::palette::{CODE_FONT, CONTROL_RADIUS, Colors, Tone};
use super::super::segmented::{Segment, SegmentLayout, Segmented};
use super::{Editing, SettingsPanel, Slot, rising};
use crate::instructions::{InstructionsChoice, InstructionsKind};

impl SettingsPanel {
    pub(super) fn instructions(
        &self,
        kind: InstructionsKind,
        editing: &Editing,
        colors: &Colors,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let id = |suffix: &str| SharedString::from(format!("{}-{suffix}", kind.label()));
        let entry = editing.instructions_entry(kind);
        let choice = editing.settings.instructions(kind);
        let is_skill = entry.skill.is_open;
        let picked = if is_skill {
            Some(2)
        } else {
            match choice {
                InstructionsChoice::DefaultFile => Some(0),
                InstructionsChoice::OtherFile(_) => Some(1),
                InstructionsChoice::Skill(_) => Some(2),
                InstructionsChoice::Off => Some(3),
                InstructionsChoice::Unreadable(_) => None,
            }
        };
        let segments = vec![
            Segment::new(
                id("default"),
                "Default file",
                cx.listener(move |this, _, _, cx| {
                    this.edit(cx, |editing| {
                        editing.instructions_entry_mut(kind).skill.is_open = false;
                        editing
                            .settings
                            .set_instructions(kind, InstructionsChoice::DefaultFile);
                    });
                }),
            ),
            Segment::new(
                id("other-file"),
                "Other file…",
                cx.listener(move |this, _, _, cx| {
                    this.edit(cx, |editing| {
                        editing.instructions_entry_mut(kind).skill.is_open = false;
                    });
                    this.pick_instructions_file(kind, cx);
                }),
            ),
            Segment::new(
                id("skill"),
                "Skill",
                cx.listener(move |this, _, _, cx| {
                    this.edit(cx, |editing| {
                        editing.instructions_entry_mut(kind).skill.is_open = true;
                    });
                }),
            ),
            Segment::new(
                id("off"),
                "Off",
                cx.listener(move |this, _, _, cx| {
                    this.edit(cx, |editing| {
                        editing.instructions_entry_mut(kind).skill.is_open = false;
                        editing
                            .settings
                            .set_instructions(kind, InstructionsChoice::Off);
                    });
                }),
            ),
        ];
        let has_problem = editing.has_problem_under(Slot::Instructions(kind));
        let control = Segmented::new(
            id("mode"),
            SegmentLayout::Row {
                segment_width: px(116.),
            },
            segments,
        )
        .picked(picked)
        .invalid(matches!(choice, InstructionsChoice::Unreadable(_)));
        let detail = if is_skill {
            Some(
                self.skill_picker(kind, editing, colors, cx)
                    .into_any_element(),
            )
        } else {
            match choice {
                    InstructionsChoice::DefaultFile => {
                        let (status, action) = if entry.default_file_exists {
                            (
                                (Tone::Success, "Found"),
                                icon_label_button(
                                    colors,
                                    id("default-open"),
                                    ButtonKind::Secondary,
                                    ButtonSize::Small,
                                    Icon::FileText,
                                    "Open",
                                ),
                            )
                        } else {
                            (
                                (Tone::Warning, "Not created yet"),
                                icon_label_button(
                                    colors,
                                    id("default-open"),
                                    ButtonKind::Tinted,
                                    ButtonSize::Small,
                                    Icon::FilePlus,
                                    "Create and open",
                                ),
                            )
                        };
                        Some(
                            file_row(colors, entry.default_file.clone(), has_problem)
                                .child(chip(colors, status.0).py_0p5().child(status.1))
                                .child(action.on_click(
                                    cx.listener(move |this, _, _, cx| {
                                        this.open_default_file(kind, cx);
                                    }),
                                ))
                                .into_any_element(),
                        )
                    }
                    InstructionsChoice::OtherFile(path) => Some(
                        file_row(colors, path.display().to_string().into(), has_problem)
                            .when(has_problem, |row| {
                                row.child(chip(colors, Tone::Danger).py_0p5().child("Missing"))
                            })
                            .child(
                                icon_label_button(
                                    colors,
                                    id("other-file-pick"),
                                    ButtonKind::Secondary,
                                    ButtonSize::Small,
                                    Icon::FolderOpen,
                                    "Choose…",
                                )
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.pick_instructions_file(kind, cx);
                                })),
                            )
                            .into_any_element(),
                    ),
                    InstructionsChoice::Skill(_) => {
                        Some(self.skill_picker(kind, editing, colors, cx).into_any_element())
                    }
                    InstructionsChoice::Off => Some(
                        div()
                            .text_sm()
                            .text_color(colors.muted_text)
                            .child(match kind {
                                InstructionsKind::Summary => "The tour writer gets no summary instructions.",
                                InstructionsKind::Review => {
                                    "The review agent gets no guidance, only how to report what it finds."
                                }
                            })
                            .into_any_element(),
                    ),
                    InstructionsChoice::Unreadable(_) => None,
                }
        };
        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(control)
            .children(detail)
            .child(Self::problem_lines(
                editing,
                Slot::Instructions(kind),
                colors,
            ))
    }

    fn skill_picker(
        &self,
        kind: InstructionsKind,
        editing: &Editing,
        colors: &Colors,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let chosen = match editing.settings.instructions(kind) {
            InstructionsChoice::Skill(name) => Some(name),
            _ => None,
        };
        let skills = editing
            .skills
            .iter()
            .enumerate()
            .map(|(index, (name, label))| {
                let picked = name.clone();
                toggle_button(
                    colors,
                    SharedString::from(format!("{}-skill-{index}", kind.label())),
                    chosen == Some(name),
                    ButtonSize::Small,
                )
                .child(label.clone())
                .aria_label(label.clone())
                .on_click(cx.listener(move |this, _, _, cx| {
                    let picked = picked.clone();
                    this.edit(cx, |editing| {
                        picked
                            .as_str()
                            .clone_into(&mut editing.instructions_entry_mut(kind).skill.text);
                        editing
                            .settings
                            .set_instructions(kind, InstructionsChoice::Skill(picked));
                    });
                }))
            });
        let skill = &editing.instructions_entry(kind).skill;
        let typed = skill.focus.as_ref().map(|focus| {
            self.text_box(
                SharedString::from(format!("{}-skill-name", kind.label())),
                focus,
                &skill.text,
                false,
                cx,
                move |editing, text| {
                    editing
                        .settings
                        .set_instructions(kind, InstructionsChoice::skill_named(&text));
                    editing.instructions_entry_mut(kind).skill.text = text;
                },
            )
        });
        rising(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .child(div().text_xs().text_color(colors.muted_text).child(
                    "Skills in ~/.claude/skills, or type any skill name, such as plugin:skill",
                ))
                .child(div().flex().flex_wrap().gap_2().children(skills))
                .children(typed),
            match kind {
                InstructionsKind::Summary => "summary-skill-rise",
                InstructionsKind::Review => "review-skill-rise",
            },
        )
    }
}

/// A file a setting points at: its icon and path, then what the caller adds (a status, an action).
fn file_row(colors: &Colors, path: SharedString, is_invalid: bool) -> gpui::Div {
    div()
        .flex()
        .items_center()
        .gap_3()
        .pl_3()
        .pr_1p5()
        .py_1p5()
        .rounded(px(CONTROL_RADIUS))
        .border_1()
        .border_color(if is_invalid {
            colors.danger
        } else {
            colors.subtle_border
        })
        .bg(colors.background)
        .child(sized_icon(Icon::FileText, colors.muted_text, px(16.)))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis_middle()
                .font_family(*CODE_FONT)
                .text_xs()
                .child(path),
        )
}
