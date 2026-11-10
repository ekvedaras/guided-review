use std::path::{Path, PathBuf};

use gpui::prelude::FluentBuilder;
use gpui::{
    Context, EventEmitter, FocusHandle, FontWeight, InteractiveElement, IntoElement, KeyDownEvent,
    ParentElement, PathPromptOptions, Render, SharedString, StatefulInteractiveElement, Styled,
    Window, div, px,
};

use crate::config::{AskEffortChoice, AskModelChoice, AskSettings};
use crate::ide::{IdeChoice, IdeTemplate};
use crate::summary_instructions::{SkillName, SummaryInstructionsChoice};
use crate::viewer::{SaveOutcome, SettingProblem, Settings};

use super::button::{ButtonKind, ButtonSize, button, toggle_button};
use super::palette::{Colors, TEXT_FONT};
use super::text_box::TextBox;
use super::text_input::Lines;
use super::title_bar::TitleBarArea;

/// Where the panel starts: the sidebar's width, so the sidebar stays beside it.
const SIDEBAR_WIDTH: f32 = 300.;
const FORM_WIDTH: f32 = 720.;

/// What the panel tells the layout. The panel writes the config; what the new settings do to the PR on
/// screen is the layout's to decide.
pub enum SettingsEvent {
    /// The config file was written, so everything that read it before is out of date.
    Saved,
}

impl EventEmitter<SettingsEvent> for SettingsPanel {}

/// The setting a red line belongs under.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Slot {
    SummaryInstructions,
    Ide,
    AskModel,
    AskEffort,
    Clone(usize),
}

struct ShownProblem {
    slot: Slot,
    message: SharedString,
}

impl ShownProblem {
    fn of(problem: &SettingProblem) -> Self {
        let slot = match problem {
            SettingProblem::Clone { row, .. } => Slot::Clone(*row),
            SettingProblem::SummaryFileGone(_)
            | SettingProblem::SummaryInstructionsUnreadable(_) => Slot::SummaryInstructions,
            SettingProblem::AskEffortUnknown(_) => Slot::AskEffort,
            SettingProblem::AskModelBlank => Slot::AskModel,
            SettingProblem::IdeTemplateBlank => Slot::Ide,
        };
        Self {
            slot,
            message: problem.message().into(),
        }
    }
}

/// A "Custom" choice with its text box. The box stays open while the reviewer types, even when the text
/// happens to equal a preset the model would read back as that preset.
#[derive(Default)]
struct CustomEntry {
    is_open: bool,
    text: String,
    focus: Option<FocusHandle>,
}

/// A config that loaded, being edited.
struct Editing {
    settings: Settings,
    skills: Vec<(SkillName, SharedString)>,
    default_file: SharedString,
    default_file_exists: bool,
    default_model: SharedString,
    default_effort: SharedString,
    ide: CustomEntry,
    model: CustomEntry,
    skill: CustomEntry,
    folder_labels: Vec<SharedString>,
    problems: Vec<ShownProblem>,
    /// Why the last Save or file creation went wrong, when it is not a problem of one setting.
    note: Option<SharedString>,
    is_confirming_discard: bool,
}

impl Editing {
    fn load(home: &Path, cx: &mut Context<SettingsPanel>) -> anyhow::Result<Self> {
        let settings = Settings::load(home)?;
        let skills = settings
            .skills()
            .into_iter()
            .map(|skill| {
                let label = SharedString::from(skill.as_str().to_owned());
                (skill, label)
            })
            .collect();
        let entry = |custom_text: Option<&str>| CustomEntry {
            is_open: custom_text.is_some(),
            text: custom_text.unwrap_or_default().to_owned(),
            focus: Some(cx.focus_handle()),
        };
        let ide = entry(settings.ide().custom_text());
        let model = entry(settings.ask_model().custom_text());
        let skill = entry(settings.summary_instructions().skill_text());
        let problems = settings
            .problems_on_open()
            .iter()
            .map(ShownProblem::of)
            .collect();
        let mut editing = Self {
            default_file: settings.default_file().display().to_string().into(),
            default_file_exists: settings.default_file_exists(),
            default_model: format!("App default ({})", AskSettings::default().model).into(),
            default_effort: format!("App default ({})", AskSettings::default().effort).into(),
            settings,
            skills,
            ide,
            model,
            skill,
            folder_labels: Vec::new(),
            problems,
            note: None,
            is_confirming_discard: false,
        };
        editing.refresh_folder_labels();
        Ok(editing)
    }

    fn refresh_folder_labels(&mut self) {
        self.folder_labels = self
            .settings
            .clones()
            .iter()
            .map(|clone| SharedString::from(clone.folder_label()))
            .collect();
    }

    /// Red lines that were up follow the edit, so the one just fixed goes away.
    fn recheck(&mut self) {
        self.note = None;
        if !self.problems.is_empty() {
            self.problems = self.settings.check().iter().map(ShownProblem::of).collect();
        }
    }

    fn problems_under(&self, slot: Slot) -> impl Iterator<Item = &SharedString> {
        self.problems
            .iter()
            .filter(move |problem| problem.slot == slot)
            .map(|problem| &problem.message)
    }
}

enum Loaded {
    Failed(SharedString),
    Ready(Box<Editing>),
}

/// The Settings panel: it covers the page area of the window, beside the sidebar. It draws a `Settings`
/// model and passes clicks on to it; what is allowed, wrong or written is the model's.
pub struct SettingsPanel {
    home: PathBuf,
    loaded: Option<Loaded>,
    focus: FocusHandle,
    owner_focus: FocusHandle,
    clone_focus: Vec<FocusHandle>,
}

impl SettingsPanel {
    pub const KEY_CONTEXT: &str = "SettingsPanel";

    pub fn new(home: PathBuf, owner_focus: FocusHandle, cx: &mut Context<Self>) -> Self {
        Self {
            home,
            loaded: None,
            focus: cx.focus_handle(),
            owner_focus,
            clone_focus: Vec::new(),
        }
    }

    pub fn is_open(&self) -> bool {
        self.loaded.is_some()
    }

    /// Reads the config again, so the panel never shows what an earlier opening left.
    pub fn open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.loaded = Some(match Editing::load(&self.home, cx) {
            Ok(editing) => Loaded::Ready(Box::new(editing)),
            Err(error) => Loaded::Failed(format!("{error:#}").into()),
        });
        self.clone_focus.clear();
        window.focus(&self.focus, cx);
        cx.notify();
    }

    /// Asks first when there are unsaved changes.
    pub fn request_close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.editing_mut() {
            Some(editing) if editing.settings.has_unsaved_changes() => {
                editing.is_confirming_discard = true;
                window.focus(&self.focus, cx);
                cx.notify();
            }
            _ => self.close(window, cx),
        }
    }

    pub fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.loaded = None;
        window.focus(&self.owner_focus, cx);
        cx.notify();
    }

    fn editing_mut(&mut self) -> Option<&mut Editing> {
        match self.loaded.as_mut()? {
            Loaded::Ready(editing) => Some(editing),
            Loaded::Failed(_) => None,
        }
    }

    fn edit(&mut self, cx: &mut Context<Self>, change: impl FnOnce(&mut Editing)) {
        if let Some(editing) = self.editing_mut() {
            change(editing);
            editing.refresh_folder_labels();
            editing.recheck();
            cx.notify();
        }
    }

    fn save(&mut self, cx: &mut Context<Self>) {
        let Some(editing) = self.editing_mut() else {
            return;
        };
        match editing.settings.save() {
            Ok(SaveOutcome::NothingChanged) => {}
            Ok(SaveOutcome::Written) => {
                editing.problems.clear();
                editing.note = None;
                cx.emit(SettingsEvent::Saved);
            }
            Ok(SaveOutcome::Refused(problems)) => {
                editing.note = None;
                editing.problems = problems.iter().map(ShownProblem::of).collect();
            }
            Err(error) => editing.note = Some(format!("{error:#}").into()),
        }
        cx.notify();
    }

    fn open_default_file(&mut self, cx: &mut Context<Self>) {
        let Some(editing) = self.editing_mut() else {
            return;
        };
        match editing.settings.create_default_file() {
            Ok(file) => {
                editing.default_file_exists = true;
                cx.open_with_system(&file);
            }
            Err(error) => editing.note = Some(format!("{error:#}").into()),
        }
        cx.notify();
    }

    /// The picker is the system's, so the answer comes back later.
    fn pick_path(
        &mut self,
        options: PathPromptOptions,
        cx: &mut Context<Self>,
        picked: impl FnOnce(&mut Editing, PathBuf) + 'static,
    ) {
        let answer = cx.prompt_for_paths(options);
        cx.spawn(async move |this, cx| {
            let picked_path = match answer.await {
                Ok(Ok(Some(paths))) => paths.into_iter().next(),
                Ok(Ok(None)) | Err(_) => None,
                Ok(Err(error)) => {
                    this.update(cx, |this, cx| {
                        if let Some(editing) = this.editing_mut() {
                            editing.note = Some(format!("{error:#}").into());
                        }
                        cx.notify();
                    })
                    .ok();
                    None
                }
            };
            if let Some(path) = picked_path {
                this.update(cx, |this, cx| {
                    this.edit(cx, |editing| picked(editing, path));
                })
                .ok();
            }
        })
        .detach();
    }

    fn pick_summary_file(&mut self, cx: &mut Context<Self>) {
        let options = PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: None,
        };
        self.pick_path(options, cx, |editing, path| {
            editing
                .settings
                .set_summary_instructions(SummaryInstructionsChoice::OtherFile(path));
        });
    }

    fn pick_clone_folder(&mut self, row: usize, cx: &mut Context<Self>) {
        let options = PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: None,
        };
        self.pick_path(options, cx, move |editing, path| {
            editing.settings.pick_clone_folder(row, path);
        });
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if event.keystroke.key != "escape" {
            return;
        }
        self.escape(window, cx);
    }

    fn escape(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.editing_mut() {
            Some(editing) if editing.is_confirming_discard => {
                editing.is_confirming_discard = false;
                cx.notify();
            }
            _ => self.request_close(window, cx),
        }
    }

    fn text_box(
        &self,
        id: impl Into<gpui::ElementId>,
        focus: &FocusHandle,
        text: &str,
        cx: &Context<Self>,
        on_edit: impl Fn(&mut Editing, String) + 'static,
    ) -> impl IntoElement {
        let (panel, cancel) = (cx.entity(), cx.entity());
        let colors = Colors::active(cx);
        div()
            .px_2()
            .h(px(28.))
            .flex()
            .items_center()
            .rounded_md()
            .border_1()
            .border_color(colors.border)
            .bg(colors.surface)
            .text_sm()
            .child(
                TextBox::new(id, focus, text, Lines::Single)
                    .on_edit(move |text, _, cx| {
                        panel.update(cx, |this, cx| {
                            this.edit(cx, |editing| on_edit(editing, text));
                        });
                    })
                    .on_cancel(move |window, cx| {
                        cancel.update(cx, |this, cx| this.escape(window, cx));
                    }),
            )
    }

    fn problem_lines(editing: &Editing, slot: Slot, colors: &Colors) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .children(editing.problems_under(slot).map(|message| {
                div()
                    .text_xs()
                    .text_color(colors.danger)
                    .child(message.clone())
            }))
    }

    fn section(
        colors: &Colors,
        title: &'static str,
        about: &'static str,
        body: impl IntoElement,
    ) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .gap_2()
            .pb_5()
            .border_b_1()
            .border_color(colors.subtle_border)
            .child(
                div()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_base()
                    .child(title),
            )
            .child(div().text_xs().text_color(colors.muted_text).child(about))
            .child(body)
    }

    fn choice(
        colors: &Colors,
        id: impl Into<gpui::ElementId>,
        label: impl IntoElement,
        is_on: bool,
    ) -> gpui::Stateful<gpui::Div> {
        toggle_button(colors, id, is_on, ButtonSize::Regular).child(label)
    }

    fn summary_instructions(
        &self,
        editing: &Editing,
        colors: &Colors,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let choice = editing.settings.summary_instructions();
        let is_default = matches!(choice, SummaryInstructionsChoice::DefaultFile);
        let other_file = match choice {
            SummaryInstructionsChoice::OtherFile(path) => Some(path),
            _ => None,
        };
        let is_off = matches!(choice, SummaryInstructionsChoice::Off);
        let is_skill = editing.skill.is_open;
        let choices = div()
            .flex()
            .flex_wrap()
            .gap_2()
            .child(
                Self::choice(colors, "summary-default", "Default file", is_default).on_click(
                    cx.listener(|this, _, _, cx| {
                        this.edit(cx, |editing| {
                            editing.skill.is_open = false;
                            editing
                                .settings
                                .set_summary_instructions(SummaryInstructionsChoice::DefaultFile);
                        });
                    }),
                ),
            )
            .child(
                Self::choice(
                    colors,
                    "summary-other-file",
                    "Other file...",
                    other_file.is_some(),
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.edit(cx, |editing| editing.skill.is_open = false);
                    this.pick_summary_file(cx);
                })),
            )
            .child(
                Self::choice(colors, "summary-skill", "Skill", is_skill).on_click(cx.listener(
                    |this, _, _, cx| this.edit(cx, |editing| editing.skill.is_open = true),
                )),
            )
            .child(
                Self::choice(colors, "summary-off", "Off", is_off).on_click(cx.listener(
                    |this, _, _, cx| {
                        this.edit(cx, |editing| {
                            editing.skill.is_open = false;
                            editing
                                .settings
                                .set_summary_instructions(SummaryInstructionsChoice::Off);
                        });
                    },
                )),
            );
        let detail = if is_default {
            let (status, action) = if editing.default_file_exists {
                ("exists", "Open it")
            } else {
                (
                    "does not exist yet, so there are no summary instructions",
                    "Create and open it",
                )
            };
            Some(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .text_sm()
                    .child(
                        div()
                            .text_color(colors.muted_text)
                            .child(format!("{} {status}", editing.default_file)),
                    )
                    .child(
                        button(
                            colors,
                            "summary-default-open",
                            ButtonKind::Secondary,
                            ButtonSize::Small,
                        )
                        .child(action)
                        .on_click(cx.listener(|this, _, _, cx| this.open_default_file(cx))),
                    )
                    .into_any_element(),
            )
        } else if let Some(path) = other_file {
            Some(
                div()
                    .text_sm()
                    .text_color(colors.muted_text)
                    .child(path.display().to_string())
                    .into_any_element(),
            )
        } else if is_skill {
            Some(self.skill_picker(editing, colors, cx).into_any_element())
        } else {
            None
        };
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(choices)
            .children(detail)
            .child(Self::problem_lines(
                editing,
                Slot::SummaryInstructions,
                colors,
            ))
    }

    fn skill_picker(
        &self,
        editing: &Editing,
        colors: &Colors,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let chosen = match editing.settings.summary_instructions() {
            SummaryInstructionsChoice::Skill(name) => Some(name),
            _ => None,
        };
        let skills = editing
            .skills
            .iter()
            .enumerate()
            .map(|(index, (name, label))| {
                let picked = name.clone();
                Self::choice(
                    colors,
                    SharedString::from(format!("summary-skill-{index}")),
                    label.clone(),
                    chosen == Some(name),
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    let picked = picked.clone();
                    this.edit(cx, |editing| {
                        picked.as_str().clone_into(&mut editing.skill.text);
                        editing
                            .settings
                            .set_summary_instructions(SummaryInstructionsChoice::Skill(picked));
                    });
                }))
            });
        let typed = editing.skill.focus.as_ref().map(|focus| {
            self.text_box(
                "summary-skill-name",
                focus,
                &editing.skill.text,
                cx,
                |editing, text| {
                    editing
                        .settings
                        .set_summary_instructions(SummaryInstructionsChoice::skill_named(&text));
                    editing.skill.text = text;
                },
            )
        });
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div().text_xs().text_color(colors.muted_text).child(
                    "Skills in ~/.claude/skills, or type any skill name, such as plugin:skill",
                ),
            )
            .child(div().flex().flex_wrap().gap_2().children(skills))
            .children(typed)
    }

    fn ide(&self, editing: &Editing, colors: &Colors, cx: &Context<Self>) -> impl IntoElement {
        let current = editing.settings.ide();
        let is_custom = editing.ide.is_open;
        let preset = |id: &'static str, label: &'static str, choice: IdeChoice| {
            let is_on = !is_custom && current == choice;
            Self::choice(colors, id, label, is_on).on_click(cx.listener(move |this, _, _, cx| {
                let choice = choice.clone();
                this.edit(cx, |editing| {
                    editing.ide.is_open = false;
                    editing.settings.set_ide(&choice);
                });
            }))
        };
        let custom_box = editing
            .ide
            .focus
            .as_ref()
            .filter(|_| is_custom)
            .map(|focus| {
                self.text_box(
                    "ide-template",
                    focus,
                    &editing.ide.text,
                    cx,
                    |editing, text| {
                        editing
                            .settings
                            .set_ide(&IdeChoice::Custom(IdeTemplate::new(text.clone())));
                        editing.ide.text = text;
                    },
                )
            });
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_2()
                    .child(preset("ide-jetbrains", "JetBrains", IdeChoice::JetBrains))
                    .child(preset("ide-vscode", "VS Code", IdeChoice::VsCode))
                    .child(preset("ide-zed", "Zed", IdeChoice::Zed))
                    .child(preset("ide-none", "None", IdeChoice::None))
                    .child(
                        Self::choice(colors, "ide-custom", "Custom", is_custom).on_click(
                            cx.listener(|this, _, _, cx| {
                                this.edit(cx, |editing| {
                                    if !editing.ide.is_open {
                                        editing.ide.is_open = true;
                                        editing.ide.text.clear();
                                        editing.settings.set_ide(&IdeChoice::Custom(
                                            IdeTemplate::new(String::new()),
                                        ));
                                    }
                                });
                            }),
                        ),
                    ),
            )
            .children(custom_box.map(|custom_box| {
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .text_xs()
                            .text_color(colors.muted_text)
                            .child("A link or a shell command. {path} and {line} become the file and the line."),
                    )
                    .child(custom_box)
            }))
            .child(Self::problem_lines(editing, Slot::Ide, colors))
    }

    fn ask(&self, editing: &Editing, colors: &Colors, cx: &Context<Self>) -> impl IntoElement {
        let model = editing.settings.ask_model();
        let is_custom = editing.model.is_open;
        let model_choice = |id: SharedString, label: SharedString, choice: AskModelChoice| {
            let is_on = !is_custom && model == choice;
            Self::choice(colors, id, label, is_on).on_click(cx.listener(move |this, _, _, cx| {
                let choice = choice.clone();
                this.edit(cx, |editing| {
                    editing.model.is_open = false;
                    editing.settings.set_ask_model(&choice);
                });
            }))
        };
        let custom_box = editing
            .model
            .focus
            .as_ref()
            .filter(|_| is_custom)
            .map(|focus| {
                self.text_box(
                    "ask-model-name",
                    focus,
                    &editing.model.text,
                    cx,
                    |editing, text| {
                        editing
                            .settings
                            .set_ask_model(&AskModelChoice::Custom(text.clone()));
                        editing.model.text = text;
                    },
                )
            });
        let effort = editing.settings.ask_effort();
        let default_effort = Self::choice(
            colors,
            "ask-effort-default",
            editing.default_effort.clone(),
            effort == AskEffortChoice::AppDefault,
        )
        .on_click(cx.listener(|this, _, _, cx| {
            this.edit(cx, |editing| {
                editing
                    .settings
                    .set_ask_effort(&AskEffortChoice::AppDefault);
            });
        }));
        let efforts = AskEffortChoice::PICKABLE.into_iter().map(|choice| {
            let label = SharedString::from(choice.config_text().unwrap_or_default().to_owned());
            let is_on = effort == choice;
            Self::choice(
                colors,
                SharedString::from(format!("ask-effort-{label}")),
                label,
                is_on,
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                let choice = choice.clone();
                this.edit(cx, |editing| editing.settings.set_ask_effort(&choice));
            }))
        });
        let model_choices = AskModelChoice::PICKABLE.into_iter().map(|choice| {
            let label = SharedString::from(choice.config_text().unwrap_or_default().to_owned());
            model_choice(
                SharedString::from(format!("ask-model-{label}")),
                label,
                choice,
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
                    .gap_2()
                    .child(div().text_sm().child("Model"))
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .gap_2()
                            .child(model_choice(
                                "ask-model-default".into(),
                                editing.default_model.clone(),
                                AskModelChoice::AppDefault,
                            ))
                            .children(model_choices)
                            .child(
                                Self::choice(colors, "ask-model-custom", "Custom", is_custom)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.edit(cx, |editing| {
                                            if !editing.model.is_open {
                                                editing.model.is_open = true;
                                                editing.model.text.clear();
                                                editing.settings.set_ask_model(
                                                    &AskModelChoice::Custom(String::new()),
                                                );
                                            }
                                        });
                                    })),
                            ),
                    )
                    .children(custom_box)
                    .child(Self::problem_lines(editing, Slot::AskModel, colors)),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(div().text_sm().child("Effort"))
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .gap_2()
                            .child(default_effort)
                            .children(efforts),
                    )
                    .child(Self::problem_lines(editing, Slot::AskEffort, colors)),
            )
    }

    fn clones(&self, editing: &Editing, colors: &Colors, cx: &Context<Self>) -> impl IntoElement {
        let rows = editing
            .settings
            .clones()
            .iter()
            .enumerate()
            .filter_map(|(row, clone)| {
                let focus = self.clone_focus.get(row)?;
                let label = editing.folder_labels.get(row)?.clone();
                Some(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .child(div().w(px(220.)).flex_none().child(self.text_box(
                                    SharedString::from(format!("clone-repo-{row}")),
                                    focus,
                                    &clone.repo,
                                    cx,
                                    move |editing, text| {
                                        editing.settings.set_clone_repo(row, text);
                                    },
                                )))
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .truncate()
                                        .text_sm()
                                        .text_color(colors.muted_text)
                                        .child(label),
                                )
                                .child(
                                    button(
                                        colors,
                                        SharedString::from(format!("clone-folder-{row}")),
                                        ButtonKind::Secondary,
                                        ButtonSize::Regular,
                                    )
                                    .child("Choose folder...")
                                    .on_click(cx.listener(
                                        move |this, _, _, cx| {
                                            this.pick_clone_folder(row, cx);
                                        },
                                    )),
                                )
                                .child(
                                    button(
                                        colors,
                                        SharedString::from(format!("clone-remove-{row}")),
                                        ButtonKind::Ghost,
                                        ButtonSize::Regular,
                                    )
                                    .child("Remove")
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
        div().flex().flex_col().gap_3().children(rows).child(
            div().flex().child(
                button(
                    colors,
                    "clone-add",
                    ButtonKind::Secondary,
                    ButtonSize::Regular,
                )
                .child("Add clone")
                .on_click(cx.listener(|this, _, _, cx| {
                    this.edit(cx, |editing| {
                        editing.settings.add_clone();
                    });
                })),
            ),
        )
    }

    fn discard_question(colors: &Colors, cx: &Context<Self>) -> impl IntoElement {
        div()
            .flex_none()
            .px_6()
            .py_3()
            .flex()
            .items_center()
            .gap_3()
            .bg(colors.warning_background)
            .border_b_1()
            .border_color(colors.subtle_border)
            .text_sm()
            .text_color(colors.warning)
            .child(
                div()
                    .flex_1()
                    .child("Close Settings and lose your changes?"),
            )
            .child(
                button(
                    colors,
                    "settings-keep-editing",
                    ButtonKind::Secondary,
                    ButtonSize::Regular,
                )
                .child("Keep editing")
                .on_click(cx.listener(|this, _, _, cx| {
                    this.edit(cx, |editing| editing.is_confirming_discard = false);
                })),
            )
            .child(
                button(
                    colors,
                    "settings-discard",
                    ButtonKind::Primary,
                    ButtonSize::Regular,
                )
                .child("Discard changes")
                .on_click(cx.listener(|this, _, window, cx| this.close(window, cx))),
            )
    }
}

impl Render for SettingsPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(loaded) = &self.loaded else {
            return div().into_any_element();
        };
        let colors = Colors::active(cx);
        let rows_needed = match loaded {
            Loaded::Ready(editing) => editing.settings.clones().len(),
            Loaded::Failed(_) => 0,
        };
        while self.clone_focus.len() < rows_needed {
            self.clone_focus.push(cx.focus_handle());
        }
        let Some(loaded) = &self.loaded else {
            return div().into_any_element();
        };
        let header = TitleBarArea::new("settings-title-bar", colors.background).child(
            div()
                .pl(px(24.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(colors.muted_text)
                .child("Settings"),
        );
        let body = match loaded {
            Loaded::Failed(reason) => div()
                .p_6()
                .text_color(colors.danger)
                .child(reason.clone())
                .into_any_element(),
            Loaded::Ready(editing) => {
                let summary = self.summary_instructions(editing, &colors, cx);
                let ide = self.ide(editing, &colors, cx);
                let ask = self.ask(editing, &colors, cx);
                let clones = self.clones(editing, &colors, cx);
                div()
                    .id("settings-form")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(
                        div()
                            .w_full()
                            .max_w(px(FORM_WIDTH))
                            .px_6()
                            .py_6()
                            .flex()
                            .flex_col()
                            .gap_5()
                            .child(Self::section(
                                &colors,
                                "Summary instructions",
                                "How the tour writer writes the reviewer summary at the start of a tour.",
                                summary,
                            ))
                            .child(Self::section(
                                &colors,
                                "Open code in",
                                "The editor a code link opens. With none, code opens on GitHub.",
                                ide,
                            ))
                            .child(Self::section(
                                &colors,
                                "Ask",
                                "The model and effort Ask uses.",
                                ask,
                            ))
                            .child(Self::section(
                                &colors,
                                "Clones",
                                "Where each repo is cloned on this machine.",
                                clones,
                            ))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_3()
                                    .child(
                                        button(
                                            &colors,
                                            "settings-save",
                                            ButtonKind::Primary,
                                            ButtonSize::Regular,
                                        )
                                        .child("Save")
                                        .on_click(cx.listener(|this, _, _, cx| this.save(cx))),
                                    )
                                    .children(editing.note.clone().map(|note| {
                                        div().text_sm().text_color(colors.danger).child(note)
                                    })),
                            ),
                    )
                    .into_any_element()
            }
        };
        let is_confirming =
            matches!(loaded, Loaded::Ready(editing) if editing.is_confirming_discard);
        div()
            .id("settings-panel")
            .key_context(Self::KEY_CONTEXT)
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key_down))
            .absolute()
            .top_0()
            .bottom_0()
            .right_0()
            .left(px(SIDEBAR_WIDTH))
            .flex()
            .flex_col()
            .bg(colors.background)
            .occlude()
            .font_family(TEXT_FONT)
            .text_sm()
            .text_color(colors.text)
            .child(header)
            .when(is_confirming, |panel| {
                panel.child(Self::discard_question(&colors, cx))
            })
            .child(body)
            .into_any_element()
    }
}
