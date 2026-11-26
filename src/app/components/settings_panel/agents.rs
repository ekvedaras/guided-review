//! The Agents page: the summary and review instructions, and the model and effort Ask uses.

use gpui::prelude::FluentBuilder;
use gpui::{
    Context, IntoElement, ParentElement, SharedString, StatefulInteractiveElement, Styled, div, px,
};

use super::super::button::{ButtonKind, ButtonSize, icon_label_button, toggle_button};
use super::super::chip::chip;
use super::super::icon::{Icon, sized_icon};
use super::super::palette::{CODE_FONT, CONTROL_RADIUS, Colors, Tone};
use super::super::segmented::{Segment, SegmentLayout, Segmented};
use super::{Editing, SettingsPanel, Slot, effort_level, rising, setting_row};
use crate::config::{AskEffortChoice, AskModelChoice};
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

    pub(super) fn ask(
        &self,
        editing: &Editing,
        colors: &Colors,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let model = editing.settings.ask_model();
        let is_custom = editing.model.is_open;
        let model_picked = if is_custom {
            Some(4)
        } else {
            Some(match model {
                AskModelChoice::AppDefault => 0,
                AskModelChoice::Opus => 1,
                AskModelChoice::Sonnet => 2,
                AskModelChoice::Haiku => 3,
                AskModelChoice::Custom(_) => 4,
            })
        };
        let model_segment = |id: &'static str, label: &'static str, choice: AskModelChoice| {
            Segment::new(
                id,
                label,
                cx.listener(move |this, _, _, cx| {
                    let choice = choice.clone();
                    this.edit(cx, |editing| {
                        editing.model.is_open = false;
                        editing.settings.set_ask_model(&choice);
                    });
                }),
            )
        };
        let model_segments = vec![
            model_segment("ask-model-default", "Default", AskModelChoice::AppDefault),
            model_segment("ask-model-opus", "opus", AskModelChoice::Opus),
            model_segment("ask-model-sonnet", "sonnet", AskModelChoice::Sonnet),
            model_segment("ask-model-haiku", "haiku", AskModelChoice::Haiku),
            Segment::new(
                "ask-model-custom",
                "Custom",
                cx.listener(|this, _, _, cx| {
                    this.edit(cx, |editing| {
                        if !editing.model.is_open {
                            editing.model.is_open = true;
                            editing.model.text =
                                editing.settings.ask_model_custom_text().to_owned();
                            editing
                                .settings
                                .set_ask_model(&AskModelChoice::Custom(editing.model.text.clone()));
                        }
                    });
                }),
            ),
        ];
        let model_note = if is_custom {
            div()
                .text_xs()
                .text_color(colors.muted_text)
                .child("Any model name claude accepts")
        } else {
            match model.shown_command() {
                Some(command) => Self::shown_text(colors, command),
                None => div()
                    .text_xs()
                    .text_color(colors.muted_text)
                    .child(editing.default_model_note.clone()),
            }
        };
        let custom_model = editing
            .model
            .focus
            .as_ref()
            .filter(|_| is_custom)
            .map(|focus| {
                rising(
                    self.text_box(
                        "ask-model-name",
                        focus,
                        &editing.model.text,
                        editing.has_problem_under(Slot::AskModel),
                        cx,
                        |editing, text| {
                            editing
                                .settings
                                .set_ask_model(&AskModelChoice::Custom(text.clone()));
                            editing.model.text = text;
                        },
                    ),
                    "ask-model-rise",
                )
            });
        let effort = editing.settings.ask_effort();
        let effort_picked = match effort {
            AskEffortChoice::AppDefault => Some(0),
            AskEffortChoice::Low => Some(1),
            AskEffortChoice::Medium => Some(2),
            AskEffortChoice::High => Some(3),
            AskEffortChoice::XHigh => Some(4),
            AskEffortChoice::Max => Some(5),
            AskEffortChoice::Unknown(_) => None,
        };
        let effort_segments =
            std::iter::once(("ask-effort-default", "Default", AskEffortChoice::AppDefault))
                .chain([
                    ("ask-effort-low", "low", AskEffortChoice::Low),
                    ("ask-effort-medium", "medium", AskEffortChoice::Medium),
                    ("ask-effort-high", "high", AskEffortChoice::High),
                    ("ask-effort-xhigh", "xhigh", AskEffortChoice::XHigh),
                    ("ask-effort-max", "max", AskEffortChoice::Max),
                ])
                .map(|(id, label, choice)| {
                    Segment::new(
                        id,
                        label,
                        cx.listener(move |this, _, _, cx| {
                            let choice = choice.clone();
                            this.edit(cx, |editing| editing.settings.set_ask_effort(&choice));
                        }),
                    )
                })
                .collect();
        let level = match effort {
            AskEffortChoice::AppDefault => editing.default_effort_level,
            ref chosen => effort_level(chosen),
        };
        let effort_note = div()
            .flex()
            .items_center()
            .gap_2()
            .text_xs()
            .text_color(colors.muted_text)
            .child(effort_meter(colors, level))
            .when(effort == AskEffortChoice::AppDefault, |note| {
                note.child(editing.default_effort_note.clone())
            });
        div()
            .flex()
            .flex_col()
            .gap_4()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(setting_row(
                        "Model",
                        model_note,
                        Segmented::new(
                            "ask-model",
                            SegmentLayout::Row {
                                segment_width: px(80.),
                            },
                            model_segments,
                        )
                        .picked(model_picked),
                    ))
                    .children(custom_model)
                    .child(Self::problem_lines(editing, Slot::AskModel, colors)),
            )
            .child(div().h_px().bg(colors.subtle_border))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(setting_row(
                        "Effort",
                        effort_note,
                        Segmented::new(
                            "ask-effort",
                            SegmentLayout::Row {
                                segment_width: px(70.),
                            },
                            effort_segments,
                        )
                        .picked(effort_picked)
                        .invalid(editing.has_problem_under(Slot::AskEffort)),
                    ))
                    .child(Self::problem_lines(editing, Slot::AskEffort, colors)),
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

/// Five bars rising from low to max, filled up to the chosen effort.
fn effort_meter(colors: &Colors, level: usize) -> impl IntoElement {
    const HEIGHTS: [f32; 5] = [4., 6., 8., 10., 12.];
    div()
        .flex()
        .items_end()
        .gap(px(2.))
        .h(px(12.))
        .children(HEIGHTS.into_iter().enumerate().map(move |(index, height)| {
            div()
                .w(px(3.))
                .h(px(height))
                .rounded(px(1.))
                .bg(if index < level {
                    colors.accent
                } else {
                    colors.border
                })
        }))
}
