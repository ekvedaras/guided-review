use std::path::{Path, PathBuf};

use gpui::prelude::FluentBuilder;
use gpui::{
    Context, EventEmitter, FocusHandle, FontWeight, InteractiveElement, IntoElement, KeyDownEvent,
    ParentElement, PathPromptOptions, Render, ScrollHandle, SharedString,
    StatefulInteractiveElement, Styled, Window, div, px, relative,
};

use crate::config::{AskEffortChoice, AskModelChoice, AskSettings};
use crate::ide::{IdeChoice, IdeTemplate};
use crate::summary_instructions::{SkillName, SummaryInstructionsChoice};
use crate::viewer::{CloneRowProblem, SaveOutcome, SettingProblem, Settings};

use super::button::{
    ButtonKind, ButtonSize, button, icon_button, icon_label_button, toggle_button,
};
use super::card::card;
use super::chip::{chip, dropping_in, notice_band};
use super::icon::{Icon, sized_icon};
use super::motion::{BOX_RISE, Edge, OVERLAY, STATE, fade_in, pop_when_shown, rise_in, slide_in};
use super::palette::{
    CARD_RADIUS, CODE_FONT, CONTROL_RADIUS, Colors, INNER_RADIUS, TEXT_FONT, Tone,
};
use super::segmented::{Segment, SegmentLayout, Segmented};
use super::text_box::{Field, TextBox};
use super::text_input::Lines;
use super::title_bar::TitleBarArea;

/// The widest a sheet over a review grows on a wide window.
const SHEET_WIDTH: f32 = 860.;
/// The column the header, the sections and the action bar's contents line up in.
const COLUMN_WIDTH: f32 = 780.;

/// What the panel tells the layout. The panel writes the config; what the new settings do to the PR on
/// screen is the layout's to decide.
pub enum SettingsEvent {
    /// The config file was written, so everything that read it before is out of date.
    Saved,
    Closed,
}

/// What the panel covers: a sheet over the whole review, sidebar too, so nothing behind it is live;
/// or a window that holds only Settings, as its whole page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placement {
    SheetOverReview,
    WholeWindow,
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

/// Which half of a clone row a problem is about, so that half wears the danger edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ClonePart {
    Repo,
    Folder,
}

impl ClonePart {
    fn of(problem: CloneRowProblem) -> Self {
        match problem {
            CloneRowProblem::NotOwnerSlashRepo | CloneRowProblem::SameRepoTwice => Self::Repo,
            CloneRowProblem::FolderGone | CloneRowProblem::NoRemoteOfRepo => Self::Folder,
        }
    }
}

struct ShownProblem {
    slot: Slot,
    clone_part: Option<ClonePart>,
    message: SharedString,
}

impl ShownProblem {
    fn of(problem: &SettingProblem) -> Self {
        let clone_part = match problem {
            SettingProblem::Clone { problem, .. } => Some(ClonePart::of(*problem)),
            _ => None,
        };
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
            clone_part,
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
    /// What the Default segments stand for, said beside the setting's name.
    default_model_note: SharedString,
    default_effort_note: SharedString,
    default_effort_level: usize,
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
        let entry = |is_open: bool, text: &str| CustomEntry {
            is_open,
            text: text.to_owned(),
            focus: Some(cx.focus_handle()),
        };
        let ide = entry(
            settings.ide().custom_text().is_some(),
            settings.ide_custom_text(),
        );
        let model = entry(
            settings.ask_model().custom_text().is_some(),
            settings.ask_model_custom_text(),
        );
        let skill_text = settings.summary_instructions().skill_text();
        let skill = entry(skill_text.is_some(), skill_text.unwrap_or_default());
        let problems = settings
            .problems_on_open()
            .iter()
            .map(ShownProblem::of)
            .collect();
        let mut editing = Self {
            default_file: settings.default_file().display().to_string().into(),
            default_file_exists: settings.default_file_exists(),
            default_model_note: format!("Default is {}", AskSettings::default().model).into(),
            default_effort_note: format!("Default is {}", AskSettings::default().effort).into(),
            default_effort_level: effort_level(&AskEffortChoice::of(Some(
                &AskSettings::default().effort,
            ))),
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

    fn has_problem_under(&self, slot: Slot) -> bool {
        self.problems.iter().any(|problem| problem.slot == slot)
    }

    fn has_clone_problem(&self, row: usize, part: ClonePart) -> bool {
        self.problems
            .iter()
            .any(|problem| problem.slot == Slot::Clone(row) && problem.clone_part == Some(part))
    }
}

enum Loaded {
    Failed(SharedString),
    Ready(Box<Editing>),
}

/// The Settings panel: it covers what its `Placement` says. It draws a `Settings`
/// model and passes clicks on to it; what is allowed, wrong or written is the model's.
pub struct SettingsPanel {
    home: PathBuf,
    loaded: Option<Loaded>,
    focus: FocusHandle,
    owner_focus: FocusHandle,
    placement: Placement,
    /// What the panel's owner has to say that is not about a setting, such as a PR that failed to open.
    notice: Option<SharedString>,
    clone_focus: Vec<FocusHandle>,
    scroll: ScrollHandle,
}

impl SettingsPanel {
    pub const KEY_CONTEXT: &str = "SettingsPanel";

    pub fn new(
        home: PathBuf,
        owner_focus: FocusHandle,
        placement: Placement,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            home,
            loaded: None,
            focus: cx.focus_handle(),
            owner_focus,
            placement,
            notice: None,
            clone_focus: Vec::new(),
            scroll: ScrollHandle::new(),
        }
    }

    pub fn show_notice(&mut self, notice: SharedString, cx: &mut Context<Self>) {
        self.notice = Some(notice);
        cx.notify();
    }

    /// The window that held only Settings now holds a PR: the panel stays as it is, with what is being
    /// edited, over that PR's page area.
    pub fn move_to(
        &mut self,
        owner_focus: FocusHandle,
        placement: Placement,
        cx: &mut Context<Self>,
    ) {
        self.owner_focus = owner_focus;
        self.placement = placement;
        self.notice = None;
        cx.notify();
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

    fn keep_editing(&mut self, cx: &mut Context<Self>) {
        self.edit(cx, |editing| editing.is_confirming_discard = false);
    }

    fn discard(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close(window, cx);
    }

    pub fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.loaded = None;
        window.focus(&self.owner_focus, cx);
        cx.emit(SettingsEvent::Closed);
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
            Some(editing) if editing.is_confirming_discard => self.keep_editing(cx),
            _ => self.request_close(window, cx),
        }
    }

    fn text_box(
        &self,
        id: impl Into<gpui::ElementId>,
        focus: &FocusHandle,
        text: &str,
        is_invalid: bool,
        cx: &Context<Self>,
        on_edit: impl Fn(&mut Editing, String) + 'static,
    ) -> Field {
        let (panel, cancel) = (cx.entity(), cx.entity());
        Field::new(focus)
            .invalid(is_invalid)
            .px_3()
            .h(ButtonSize::Regular.height())
            .flex()
            .items_center()
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

    /// A preset's template or command, to read and not to edit.
    fn shown_text(colors: &Colors, text: impl Into<SharedString>) -> gpui::Div {
        div()
            .text_xs()
            .font_family(CODE_FONT)
            .text_color(colors.muted_text)
            .child(text.into())
    }

    fn problem_lines(editing: &Editing, slot: Slot, colors: &Colors) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .gap_1()
            .children(editing.problems_under(slot).map(|message| {
                div()
                    .flex()
                    .items_center()
                    .gap_1p5()
                    .text_xs()
                    .text_color(colors.danger)
                    .child(sized_icon(Icon::CircleAlert, colors.danger, px(13.)))
                    .child(message.clone())
            }))
    }

    fn section(
        colors: &Colors,
        icon: Icon,
        title: &'static str,
        about: &'static str,
        body: impl IntoElement,
    ) -> impl IntoElement {
        column(
            card(colors)
                .w_full()
                .p_6()
                .flex()
                .flex_col()
                .gap_5()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_4()
                        .child(icon_tile(colors, icon))
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap_0p5()
                                .child(
                                    div()
                                        .text_base()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .child(title),
                                )
                                .child(div().text_sm().text_color(colors.muted_text).child(about)),
                        ),
                )
                .child(body),
        )
    }

    fn summary_instructions(
        &self,
        editing: &Editing,
        colors: &Colors,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let choice = editing.settings.summary_instructions();
        let is_skill = editing.skill.is_open;
        let picked = if is_skill {
            Some(2)
        } else {
            match choice {
                SummaryInstructionsChoice::DefaultFile => Some(0),
                SummaryInstructionsChoice::OtherFile(_) => Some(1),
                SummaryInstructionsChoice::Skill(_) => Some(2),
                SummaryInstructionsChoice::Off => Some(3),
                SummaryInstructionsChoice::Unreadable(_) => None,
            }
        };
        let segments = vec![
            Segment::new(
                "summary-default",
                "Default file",
                cx.listener(|this, _, _, cx| {
                    this.edit(cx, |editing| {
                        editing.skill.is_open = false;
                        editing
                            .settings
                            .set_summary_instructions(SummaryInstructionsChoice::DefaultFile);
                    });
                }),
            ),
            Segment::new(
                "summary-other-file",
                "Other file…",
                cx.listener(|this, _, _, cx| {
                    this.edit(cx, |editing| editing.skill.is_open = false);
                    this.pick_summary_file(cx);
                }),
            ),
            Segment::new(
                "summary-skill",
                "Skill",
                cx.listener(|this, _, _, cx| this.edit(cx, |editing| editing.skill.is_open = true)),
            ),
            Segment::new(
                "summary-off",
                "Off",
                cx.listener(|this, _, _, cx| {
                    this.edit(cx, |editing| {
                        editing.skill.is_open = false;
                        editing
                            .settings
                            .set_summary_instructions(SummaryInstructionsChoice::Off);
                    });
                }),
            ),
        ];
        let has_problem = editing.has_problem_under(Slot::SummaryInstructions);
        let control = Segmented::new(
            "summary-mode",
            SegmentLayout::Row {
                segment_width: px(116.),
            },
            segments,
        )
        .picked(picked)
        .invalid(matches!(choice, SummaryInstructionsChoice::Unreadable(_)));
        let detail =
            if is_skill {
                Some(self.skill_picker(editing, colors, cx).into_any_element())
            } else {
                match choice {
                    SummaryInstructionsChoice::DefaultFile => {
                        let (status, action) = if editing.default_file_exists {
                            (
                                (Tone::Success, "Found"),
                                icon_label_button(
                                    colors,
                                    "summary-default-open",
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
                                    "summary-default-open",
                                    ButtonKind::Tinted,
                                    ButtonSize::Small,
                                    Icon::FilePlus,
                                    "Create and open",
                                ),
                            )
                        };
                        Some(
                            file_row(colors, editing.default_file.clone(), has_problem)
                                .child(chip(colors, status.0).py_0p5().child(status.1))
                                .child(action.on_click(
                                    cx.listener(|this, _, _, cx| this.open_default_file(cx)),
                                ))
                                .into_any_element(),
                        )
                    }
                    SummaryInstructionsChoice::OtherFile(path) => Some(
                        file_row(colors, path.display().to_string().into(), has_problem)
                            .when(has_problem, |row| {
                                row.child(chip(colors, Tone::Danger).py_0p5().child("Missing"))
                            })
                            .child(
                                icon_label_button(
                                    colors,
                                    "summary-other-file-pick",
                                    ButtonKind::Secondary,
                                    ButtonSize::Small,
                                    Icon::FolderOpen,
                                    "Choose…",
                                )
                                .on_click(cx.listener(|this, _, _, cx| this.pick_summary_file(cx))),
                            )
                            .into_any_element(),
                    ),
                    SummaryInstructionsChoice::Skill(_) => {
                        Some(self.skill_picker(editing, colors, cx).into_any_element())
                    }
                    SummaryInstructionsChoice::Off => Some(
                        div()
                            .text_sm()
                            .text_color(colors.muted_text)
                            .child("The tour writer gets no summary instructions.")
                            .into_any_element(),
                    ),
                    SummaryInstructionsChoice::Unreadable(_) => None,
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
                toggle_button(
                    colors,
                    SharedString::from(format!("summary-skill-{index}")),
                    chosen == Some(name),
                    ButtonSize::Small,
                )
                .child(label.clone())
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
                false,
                cx,
                |editing, text| {
                    editing
                        .settings
                        .set_summary_instructions(SummaryInstructionsChoice::skill_named(&text));
                    editing.skill.text = text;
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
            "summary-skill-rise",
        )
    }

    fn ide(&self, editing: &Editing, colors: &Colors, cx: &Context<Self>) -> impl IntoElement {
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
                            .font_family(CODE_FONT),
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
                    .child("Code links open the file on GitHub.")
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

    fn ask(&self, editing: &Editing, colors: &Colors, cx: &Context<Self>) -> impl IntoElement {
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

    fn clones(&self, editing: &Editing, colors: &Colors, cx: &Context<Self>) -> impl IntoElement {
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
                                                    path.font_family(CODE_FONT)
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

    /// Pinned under the form, so Save is always in reach: what state the edits are in on the left,
    /// the ways out on the right. While the discard question is up, it takes the bar.
    fn action_bar(
        &self,
        editing: &Editing,
        colors: &Colors,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let content = if editing.is_confirming_discard {
            slide_in(
                bar_row()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .items_center()
                            .gap_2()
                            .text_color(colors.text)
                            .child(sized_icon(Icon::CircleAlert, colors.warning, px(16.)))
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
                        .on_click(cx.listener(|this, _, _, cx| this.keep_editing(cx))),
                    )
                    .child(
                        icon_label_button(
                            colors,
                            "settings-discard",
                            ButtonKind::Danger,
                            ButtonSize::Large,
                            Icon::Trash,
                            "Discard changes",
                        )
                        .bg(colors.danger_background)
                        .border_color(colors.danger_border)
                        .on_click(cx.listener(|this, _, window, cx| this.discard(window, cx))),
                    ),
                "settings-bar-question",
                STATE,
                Edge::Bottom,
                BOX_RISE,
            )
            .into_any_element()
        } else {
            let state = if let Some(note) = editing.note.clone() {
                Some(
                    bar_state(colors, colors.danger)
                        .child(note)
                        .into_any_element(),
                )
            } else if !editing.problems.is_empty() {
                let count = editing.problems.len();
                Some(
                    bar_state(colors, colors.danger)
                        .child(if count == 1 {
                            "1 setting to fix before saving".to_owned()
                        } else {
                            format!("{count} settings to fix before saving")
                        })
                        .into_any_element(),
                )
            } else if editing.settings.has_unsaved_changes() {
                Some(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .text_color(colors.text)
                        .child(div().size(px(8.)).rounded_full().bg(colors.accent))
                        .child("Unsaved changes")
                        .into_any_element(),
                )
            } else {
                None
            };
            bar_row()
                .child(div().flex_1().min_w_0().children(state))
                .child(
                    button(
                        colors,
                        "settings-close",
                        ButtonKind::Ghost,
                        ButtonSize::Regular,
                    )
                    .child("Close")
                    .on_click(cx.listener(|this, _, window, cx| this.request_close(window, cx))),
                )
                .child(
                    icon_label_button(
                        colors,
                        "settings-save",
                        ButtonKind::Primary,
                        ButtonSize::Large,
                        Icon::Save,
                        "Save",
                    )
                    .min_w(px(112.))
                    .on_click(cx.listener(|this, _, _, cx| this.save(cx))),
                )
                .into_any_element()
        };
        div()
            .flex_none()
            .flex()
            .justify_center()
            .px_8()
            .bg(colors.overlay)
            .border_t_1()
            .border_color(colors.border)
            .shadow(colors.bar_shadow())
            .child(content)
    }

    /// A made-up config being edited, for the pictures: a custom IDE, another model and effort,
    /// two clones, one of them wrong, and nothing saved yet.
    #[cfg(feature = "snapshots")]
    pub fn stage_sample_edits(&mut self, cx: &mut Context<Self>) {
        const TEMPLATE: &str = "code --goto {path}:{line}";
        self.edit(cx, |editing| {
            editing.ide.is_open = true;
            TEMPLATE.clone_into(&mut editing.ide.text);
            editing
                .settings
                .set_ide(&IdeChoice::Custom(IdeTemplate::new(TEMPLATE.to_owned())));
            editing.settings.set_ask_model(&AskModelChoice::Sonnet);
            editing.settings.set_ask_effort(&AskEffortChoice::High);
            for (repo, folder) in [
                ("acme/widgets", "~/code/widgets"),
                ("acme/billing-service", "~/code/archive/billing-service"),
            ] {
                let row = editing.settings.add_clone();
                editing.settings.set_clone_repo(row, repo.to_owned());
                editing
                    .settings
                    .set_clone_folder(row, PathBuf::from(folder));
            }
        });
        if let Some(editing) = self.editing_mut()
            && let Some(clone) = editing.settings.clones().get(1).cloned()
        {
            editing.problems = vec![ShownProblem::of(&SettingProblem::Clone {
                row: 1,
                clone,
                problem: CloneRowProblem::FolderGone,
            })];
        }
        cx.notify();
    }

    /// Scrolls the form to its last section, for the pictures.
    #[cfg(feature = "snapshots")]
    pub fn scroll_to_end(&self) {
        self.scroll.scroll_to_item(SECTION_COUNT);
    }
}

/// The form's header and its four sections, which the scroll counts as items.
#[cfg(feature = "snapshots")]
const SECTION_COUNT: usize = 4;

/// The Settings column: as wide as the window allows, up to `COLUMN_WIDTH`, centred.
fn column(content: impl IntoElement) -> gpui::Div {
    div()
        .w_full()
        .px_8()
        .flex()
        .justify_center()
        .child(div().w_full().max_w(px(COLUMN_WIDTH)).child(content))
}

fn bar_row() -> gpui::Div {
    div()
        .w_full()
        .max_w(px(COLUMN_WIDTH))
        .py_3()
        .flex()
        .items_center()
        .gap_3()
}

fn bar_state(colors: &Colors, tone: gpui::Rgba) -> gpui::Div {
    div()
        .flex()
        .items_center()
        .gap_2()
        .min_w_0()
        .truncate()
        .text_color(tone)
        .child(sized_icon(Icon::CircleAlert, colors.danger, px(16.)))
}

/// An inner part of a section opening (a custom box, the skill picker) rises into place.
fn rising(content: impl IntoElement + Styled + 'static, id: &'static str) -> impl IntoElement {
    slide_in(content, id, STATE, Edge::Bottom, BOX_RISE)
}

/// A section's mark: its icon on a soft iris-to-azure tile.
fn icon_tile(colors: &Colors, icon: Icon) -> impl IntoElement {
    div()
        .flex_none()
        .size(px(36.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(CONTROL_RADIUS))
        .border_1()
        .border_color(colors.current_border)
        .bg(colors.icon_tile.diagonal())
        .child(sized_icon(icon, colors.accent, px(18.)))
}

/// A setting's name with a note under it on the left, its control on the right; on a narrow
/// window the control goes under.
fn setting_row(name: &'static str, note: gpui::Div, control: impl IntoElement) -> impl IntoElement {
    div()
        .flex()
        .flex_wrap()
        .items_center()
        .justify_between()
        .gap_x_6()
        .gap_y_3()
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(div().font_weight(FontWeight::MEDIUM).child(name))
                .child(note),
        )
        .child(control)
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
                .font_family(CODE_FONT)
                .text_xs()
                .child(path),
        )
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

/// How much thinking an effort asks for, from 1 (low) to 5 (max); 0 for an effort Claude does not know.
fn effort_level(effort: &AskEffortChoice) -> usize {
    AskEffortChoice::PICKABLE
        .iter()
        .position(|pickable| pickable == effort)
        .map_or(0, |index| index + 1)
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
        let close = icon_button(
            &colors,
            "settings-close-sheet",
            ButtonKind::Ghost,
            ButtonSize::Regular,
            Icon::Close,
            "Close Settings",
        )
        .on_click(cx.listener(|this, _, window, cx| this.request_close(window, cx)));
        let header = div()
            .flex_none()
            .w_full()
            .pt_5()
            .pb_4()
            .border_b_1()
            .border_color(colors.subtle_border)
            .child(column(
                div()
                    .flex()
                    .items_start()
                    .gap_3()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(
                                div()
                                    .text_2xl()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child("Settings"),
                            )
                            .child(
                                div()
                                    .text_color(colors.muted_text)
                                    .child("How tours are summarised, where code opens, how Ask thinks and where your clones live."),
                            ),
                    )
                    .child(close),
            ));
        let (body, bar) = match loaded {
            Loaded::Failed(reason) => (
                div()
                    .flex_1()
                    .pt_6()
                    .child(column(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .text_color(colors.danger)
                            .child(sized_icon(Icon::CircleAlert, colors.danger, px(16.)))
                            .child(reason.clone()),
                    ))
                    .into_any_element(),
                None,
            ),
            Loaded::Ready(editing) => {
                let summary = self.summary_instructions(editing, &colors, cx);
                let ide = self.ide(editing, &colors, cx);
                let ask = self.ask(editing, &colors, cx);
                let clones = self.clones(editing, &colors, cx);
                (
                    div()
                        .id("settings-form")
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scroll()
                        .track_scroll(&self.scroll)
                        .flex()
                        .flex_col()
                        .gap_5()
                        .pt_6()
                        .pb_10()
                        .child(Self::section(
                            &colors,
                            Icon::FileText,
                            "Summary instructions",
                            "How the tour writer writes the reviewer summary at the start of a tour.",
                            summary,
                        ))
                        .child(Self::section(
                            &colors,
                            Icon::Code,
                            "Open code in",
                            "The editor a code link opens. With none, code opens on GitHub.",
                            ide,
                        ))
                        .child(Self::section(
                            &colors,
                            Icon::Sparkles,
                            "Ask",
                            "The model and effort Ask uses.",
                            ask,
                        ))
                        .child(Self::section(
                            &colors,
                            Icon::FolderGit,
                            "Clones",
                            "Where each repo is cloned on this machine.",
                            clones,
                        ))
                        .into_any_element(),
                    Some(self.action_bar(editing, &colors, cx)),
                )
            }
        };
        let notice = self.notice.clone().map(|notice| {
            dropping_in(
                notice_band(&colors, Tone::Danger).py_3().child(notice),
                "settings-notice-band",
            )
        });
        let panel = div()
            .id("settings-panel")
            .key_context(Self::KEY_CONTEXT)
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key_down))
            .flex()
            .flex_col()
            .font_family(TEXT_FONT)
            .text_sm()
            .text_color(colors.text);
        match self.placement {
            Placement::WholeWindow => fade_in(
                panel
                    .absolute()
                    .inset_0()
                    .bg(colors.background)
                    .occlude()
                    .child(TitleBarArea::new("settings-title-bar", colors.background))
                    .child(header)
                    .children(notice)
                    .child(body)
                    .children(bar),
                "settings-panel-fade",
                OVERLAY,
            )
            .into_any_element(),
            Placement::SheetOverReview => div()
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .occlude()
                .child(fade_in(
                    div()
                        .id("settings-scrim")
                        .absolute()
                        .inset_0()
                        .bg(colors.scrim)
                        .on_click(
                            cx.listener(|this, _, window, cx| this.request_close(window, cx)),
                        ),
                    "settings-scrim-fade",
                    STATE,
                ))
                .child(rise_in(
                    panel
                        .relative()
                        .w_full()
                        .max_w(px(SHEET_WIDTH))
                        .h(relative(0.9))
                        .mx_8()
                        .overflow_hidden()
                        .bg(colors.overlay)
                        .border_1()
                        .border_color(colors.border)
                        .rounded(px(CARD_RADIUS))
                        .shadow(colors.overlay_shadow())
                        .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .child(header)
                        .children(notice)
                        .child(body)
                        .children(bar),
                    "settings-sheet-rise",
                ))
                .into_any_element(),
        }
    }
}
